# Orchyn patches to headless_chrome 1.0.21

Vendored from crates.io (`headless_chrome` 1.0.21, MIT, see LICENSE.md) via
`[patch.crates-io]` in the workspace `Cargo.toml`. Only `src/` changes below;
everything else is upstream. Search for `ORCHYN PATCH`.

1. `src/browser/mod.rs` — new-tab handles (`Tab::new`) are built on their own
   thread instead of inside the browser-level event loop, and a failure no
   longer stops that loop.
2. `src/browser/tab/mod.rs` — while `Page.enable` is pending on a new tab, a
   helper thread keeps sending `Page.handleJavaScriptDialog(accept)` to it.

Why: a same-origin popup shares its opener's event loop. If it calls `alert()`
while loading, both pages freeze; `Tab::new` then blocks on `Page.enable`
inside the single browser event thread, and every later call deadlocks
(including the click that opened the popup). Upstream has no way to pause new
targets before their scripts run.
3. Full Chrome popup freeze (`alert()` in a same-origin popup during load). The
   popup's renderer is shared with the opener, so the opener freezes too, and
   the dialog is not tracked until Page is enabled on the popup. With full Chrome
   the popup's `Page.enable` never reaches a blocked renderer, so the dialog stays
   open. The fix pauses each new page before its scripts run, enables Page on it,
   then resumes it:
   - `src/browser/mod.rs` (`Browser::new`) — browser-level
     `Target.setAutoAttach { waitForDebuggerOnStart, flatten, filter: page }`.
     Only the flattened protocol is allowed here.
   - `src/browser/transport/web_socket_connection.rs` — a message with a top-level
     `sessionId` (flattened session) is re-wrapped as `Target.receivedMessageFromTarget`,
     so the existing routing handles it. `flat_session_id` does the check.
   - `src/browser/transport/mod.rs` — records auto-attached page sessions
     (`take_auto_attached`); `send_to_session` sends flattened sessions directly with a
     `sessionId` attribute, since `Target.sendMessageToTarget` is refused for them.
     `call_method_on_target_detached` / `await_detached_call` queue a call without
     waiting for the reply.
   - `src/browser/tab/mod.rs` — `Tab::new_created` (used for `TargetCreated`) takes the
     auto-attach session, waiting up to 2s. For a paused session it queues
     `Page.enable` and the lifecycle call, starts the event thread, sends
     `Runtime.runIfWaitingForDebugger`, then awaits the replies. `Tab::new` keeps the
     old path for pages that were not auto-attached.
   - `src/browser/tab/mod.rs` (event loop) — a `Page.javascriptDialogOpening` that
     arrives while no event listener is registered is accepted, so a popup alert
     that opens before the adapter attaches its dialog listener does not hold the page.
