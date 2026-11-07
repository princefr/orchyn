use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use anyhow::Result;

use thiserror::Error;

use log::{error, info, trace, warn};

use url::Url;
use waiting_call_registry::WaitingCallRegistry;
use web_socket_connection::WebSocketConnection;

use crate::protocol::cdp::{Target, types::Event, types::Method};

use crate::types::{CallId, Message, parse_raw_message, parse_response};

use crate::util;

mod waiting_call_registry;
mod web_socket_connection;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

pub enum MethodDestination {
    Target(SessionId),
    Browser,
}

impl SessionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SessionId {
    fn from(session_id: String) -> Self {
        Self(session_id)
    }
}

#[derive(Debug, Eq, PartialEq, Hash)]
enum ListenerId {
    SessionId(SessionId),
    Browser,
}

type Listeners = Arc<Mutex<HashMap<ListenerId, Sender<Event>>>>;

#[derive(Debug)]
pub struct Transport {
    web_socket_connection: Arc<WebSocketConnection>,
    waiting_call_registry: Arc<WaitingCallRegistry>,
    listeners: Listeners,
    open: Arc<AtomicBool>,
    call_id_counter: Arc<AtomicU32>,
    loop_shutdown_tx: Mutex<mpsc::SyncSender<()>>,
    idle_browser_timeout: Duration,
    // ORCHYN PATCH: page sessions the browser auto-attached, by target id -> (session, waiting for debugger)
    auto_attached: AutoAttached,
}

// ORCHYN PATCH: pending page auto-attaches, and the flattened sessions among them.
#[derive(Default, Debug)]
struct AutoAttachState {
    pending: HashMap<String, (SessionId, bool)>,
    flat: std::collections::HashSet<String>,
}
type AutoAttached = Arc<Mutex<AutoAttachState>>;

#[derive(Debug, Error)]
#[error("Unable to make method calls because underlying connection is closed")]
pub struct ConnectionClosed {}

impl Transport {
    pub fn new(
        ws_url: Url,
        process_id: Option<u32>,
        idle_browser_timeout: Duration,
    ) -> Result<Self> {
        let (messages_tx, messages_rx) = mpsc::channel();
        let web_socket_connection =
            Arc::new(WebSocketConnection::new(&ws_url, process_id, messages_tx)?);

        let waiting_call_registry = Arc::new(WaitingCallRegistry::new());

        let listeners = Arc::new(Mutex::new(HashMap::new()));

        let open = Arc::new(AtomicBool::new(true));

        let (shutdown_tx, shutdown_rx) = mpsc::sync_channel(100);

        let guarded_shutdown_tx = Mutex::new(shutdown_tx);
        let auto_attached: AutoAttached = Arc::new(Mutex::new(AutoAttachState::default()));

        Self::handle_incoming_messages(
            messages_rx,
            Arc::clone(&waiting_call_registry),
            Arc::clone(&listeners),
            Arc::clone(&open),
            Arc::clone(&web_socket_connection),
            shutdown_rx,
            process_id,
            idle_browser_timeout,
            Arc::clone(&auto_attached),
        );

        Ok(Self {
            web_socket_connection,
            waiting_call_registry,
            listeners,
            open,
            call_id_counter: Arc::new(AtomicU32::new(0)),
            loop_shutdown_tx: guarded_shutdown_tx,
            idle_browser_timeout,
            auto_attached,
        })
    }

    // ORCHYN PATCH: take the auto-attached session for a page, waiting up to `wait` for it.
    pub fn take_auto_attached(&self, target_id: &str, wait: Duration) -> Option<(SessionId, bool)> {
        let start = std::time::Instant::now();
        loop {
            if let Some(found) = self.auto_attached.lock().unwrap().pending.remove(target_id) {
                return Some(found);
            }
            if start.elapsed() >= wait {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Returns a number based on thread-safe unique counter, incrementing it so that the
    /// next CallId is different.
    pub fn unique_call_id(&self) -> CallId {
        self.call_id_counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn call_method<C>(
        &self,
        method: C,
        destination: MethodDestination,
    ) -> Result<C::ReturnObject>
    where
        C: Method + serde::Serialize,
    {
        // TODO: use get_mut to get exclusive access for entire block... maybe.
        if !self.open.load(Ordering::SeqCst) {
            return Err(ConnectionClosed {}.into());
        }
        let call_id = self.unique_call_id();
        let call = method.to_method_call(call_id);

        let message_text = serde_json::to_string(&call)?;

        let response_rx = self.waiting_call_registry.register_call(call.id);

        match destination {
            MethodDestination::Target(session_id) => {
                trace!(
                    "Msg to tab: {}",
                    message_text.chars().take(300).collect::<String>()
                );
                if let Err(e) = self.send_to_session(&session_id, &message_text) {
                    warn!("Failed to call method on browser: {e:?}");
                    self.waiting_call_registry.unregister_call(call.id);
                    trace!("Unregistered callback: {:?}", call.id);
                    return Err(e);
                }
            }
            MethodDestination::Browser => {
                if let Err(e) = self.web_socket_connection.send_message(&message_text) {
                    self.waiting_call_registry.unregister_call(call.id);
                    return Err(e);
                }
                trace!("sent method call to browser via websocket");
            }
        }

        let params_string = format!("{:?}", call.get_params());
        trace!(
            "waiting for response from call registry: {} {:?}",
            &call_id,
            params_string.chars().take(400).collect::<String>()
        );

        let response_result = util::Wait::new(self.idle_browser_timeout, Duration::from_millis(5))
            .until(|| response_rx.try_recv().ok());
        trace!("received response for: {} {:?}", &call_id, params_string);
        parse_response::<C::ReturnObject>((response_result?)?)
    }

    pub fn call_method_on_target<C>(
        &self,
        session_id: SessionId,
        method: C,
    ) -> Result<C::ReturnObject>
    where
        C: Method + serde::Serialize,
    {
        // TODO: remove clone
        self.call_method(method, MethodDestination::Target(session_id))
    }

    // ORCHYN PATCH: send one call to a session. A flattened session (browser-level
    // auto-attach) takes the message directly with a sessionId attribute. Other
    // sessions go through Target.sendMessageToTarget.
    fn send_to_session(&self, session_id: &SessionId, message_text: &str) -> Result<()> {
        let is_flat = self.auto_attached.lock().unwrap().flat.contains(&session_id.0);
        if is_flat {
            let mut value: serde_json::Value = serde_json::from_str(message_text)?;
            value["sessionId"] = serde_json::Value::String(session_id.0.clone());
            self.web_socket_connection.send_message(&value.to_string())
        } else {
            self.call_method_on_browser(Target::SendMessageToTarget {
                target_id: None,
                session_id: Some(session_id.0.clone()),
                message: message_text.to_string(),
            })
            .map(|_| ())
        }
    }

    // ORCHYN PATCH: send a call to a target session and return its reply channel
    // without waiting for the reply (see Tab::new). Pair with await_detached_call.
    pub fn call_method_on_target_detached<C>(
        &self,
        session_id: SessionId,
        method: C,
    ) -> Result<Receiver<Result<crate::types::Response>>>
    where
        C: Method + serde::Serialize,
    {
        if !self.open.load(Ordering::SeqCst) {
            return Err(ConnectionClosed {}.into());
        }
        let call_id = self.unique_call_id();
        let call = method.to_method_call(call_id);
        let message_text = serde_json::to_string(&call)?;
        let response_rx = self.waiting_call_registry.register_call(call.id);
        let sent = self.send_to_session(&session_id, &message_text);
        if let Err(e) = sent {
            self.waiting_call_registry.unregister_call(call.id);
            return Err(e);
        }
        Ok(response_rx)
    }

    // ORCHYN PATCH: wait for the reply to a call sent by call_method_on_target_detached.
    pub fn await_detached_call<C>(
        &self,
        response_rx: Receiver<Result<crate::types::Response>>,
    ) -> Result<C::ReturnObject>
    where
        C: Method,
    {
        let response_result = util::Wait::new(self.idle_browser_timeout, Duration::from_millis(5))
            .until(|| response_rx.try_recv().ok());
        parse_response::<C::ReturnObject>((response_result?)?)
    }

    pub fn call_method_on_browser<C>(&self, method: C) -> Result<C::ReturnObject>
    where
        C: Method + serde::Serialize,
    {
        self.call_method(method, MethodDestination::Browser)
    }

    pub fn listen_to_browser_events(&self) -> Receiver<Event> {
        let (events_tx, events_rx) = mpsc::channel();

        let mut listeners = self.listeners.lock().unwrap();
        listeners.insert(ListenerId::Browser, events_tx);

        events_rx
    }

    pub fn listen_to_target_events(&self, session_id: SessionId) -> Receiver<Event> {
        let (events_tx, events_rx) = mpsc::channel();

        let mut listeners = self.listeners.lock().unwrap();
        listeners.insert(ListenerId::SessionId(session_id), events_tx);

        events_rx
    }

    pub fn shutdown(&self) {
        self.web_socket_connection.shutdown();
        let shutdown_tx = self.loop_shutdown_tx.lock().unwrap();
        let _ = shutdown_tx.send(());
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_incoming_messages(
        messages_rx: Receiver<Message>,
        waiting_call_registry: Arc<WaitingCallRegistry>,
        listeners: Listeners,
        open: Arc<AtomicBool>,
        conn: Arc<WebSocketConnection>,
        shutdown_rx: Receiver<()>,
        process_id: Option<u32>,
        idle_browser_timeout: Duration,
        auto_attached: AutoAttached,
    ) {
        trace!("Starting handle_incoming_messages");
        std::thread::spawn(move || {
            trace!("Inside handle_incoming_messages thread");
            // this iterator calls .recv() under the hood, so can block thread forever
            // hence need for Connection Shutdown
            loop {
                match shutdown_rx.try_recv() {
                    Ok(()) | Err(TryRecvError::Disconnected) => {
                        info!("Transport incoming message loop loop received shutdown message");
                        break;
                    }
                    Err(TryRecvError::Empty) => {}
                }
                match messages_rx.recv_timeout(idle_browser_timeout) {
                    Err(recv_timeout_error) => {
                        match recv_timeout_error {
                            RecvTimeoutError::Timeout => {
                                error!(
                                    "Transport loop got a timeout while listening for messages (Chrome #{process_id:?})",
                                );
                            }
                            RecvTimeoutError::Disconnected => {
                                error!(
                                    "Transport loop got disconnected from WS's sender (Chrome #{process_id:?})",
                                );
                            }
                        }
                        break;
                    }
                    Ok(message) => match message {
                        Message::ConnectionShutdown => {
                            info!("Received shutdown message");
                            break;
                        }
                        Message::Response(response_to_browser_method_call) => {
                            if waiting_call_registry
                                .resolve_call(response_to_browser_method_call)
                                .is_err()
                            {
                                warn!(
                                    "The browser registered a call but then closed its receiving channel"
                                );
                                break;
                            }
                        }

                        Message::Event(browser_event) => match browser_event {
                            Event::ReceivedMessageFromTarget(target_message_event) => {
                                let session_id = target_message_event.params.session_id.into();
                                let raw_message = target_message_event.params.message;

                                let msg_res = parse_raw_message(&raw_message);
                                match msg_res {
                                    Ok(target_message) => match target_message {
                                        Message::Event(target_event) => {
                                            if let Some(tx) = listeners
                                                .lock()
                                                .unwrap()
                                                .get(&ListenerId::SessionId(session_id))
                                            {
                                                tx.send(target_event)
                                                    .expect("Couldn't send event to listener");
                                            }
                                        }

                                        Message::Response(resp) => {
                                            if waiting_call_registry.resolve_call(resp).is_err() {
                                                warn!(
                                                    "The browser registered a call but then closed its receiving channel"
                                                );
                                                break;
                                            }
                                        }
                                        Message::ConnectionShutdown => {}
                                    },
                                    Err(e) => {
                                        trace!(
                                            "Message from target isn't recognised: {:?} - {}",
                                            &raw_message, e,
                                        );
                                    }
                                }
                            }

                            _ => {
                                // ORCHYN PATCH: record page sessions the browser auto-attached
                                if let Event::AttachedToTarget(ev) = &browser_event {
                                    if ev.params.target_info.Type == "page" {
                                        let session_id = SessionId::from(ev.params.session_id.clone());
                                        let mut state = auto_attached.lock().unwrap();
                                        state.flat.insert(session_id.0.clone());
                                        state.pending.insert(
                                            ev.params.target_info.target_id.clone(),
                                            (session_id, ev.params.waiting_for_debugger),
                                        );
                                    }
                                }
                                if let Some(tx) =
                                    listeners.lock().unwrap().get(&ListenerId::Browser)
                                {
                                    if let Err(err) = tx.send(browser_event.clone()) {
                                        let event_string = format!("{browser_event:?}");
                                        warn!(
                                            "Couldn't send browser an event: {:?}\n{:?}",
                                            event_string.chars().take(400).collect::<String>(),
                                            err
                                        );
                                        break;
                                    }
                                }
                            }
                        },
                    },
                }
            }

            info!("Shutting down message handling loop");

            // Need to do this because otherwise WS thread might block forever
            conn.shutdown();

            open.store(false, Ordering::SeqCst);
            waiting_call_registry.cancel_outstanding_method_calls();
            let mut listeners = listeners.lock().unwrap();
            *listeners = HashMap::new();
            info!("cleared listeners, I think");
        });
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        info!("dropping transport");
    }
}
