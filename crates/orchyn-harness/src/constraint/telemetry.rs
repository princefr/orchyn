use orchyn_core::{RawTelemetry, TelemetrySnapshot};

const KEEP_RECENT: usize = 3;

/// Compresses raw browser telemetry into the small snapshot the model sees.
/// Only the most-recent few errors and 4xx/5xx requests survive — older
/// noise is dropped so the prompt stays compact.
pub fn compress_telemetry(raw: RawTelemetry) -> TelemetrySnapshot {
    let console_errors: Vec<String> = raw
        .console_logs
        .into_iter()
        .filter(|l| matches!(l.level.as_str(), "error" | "warn"))
        .map(|l| format!("[{}] {}", l.level, l.message))
        .rev()
        .take(KEEP_RECENT)
        .collect();

    let network_4xx_5xx: Vec<String> = raw
        .network_requests
        .into_iter()
        .filter(|r| r.status >= 400)
        .map(|r| format!("{} {} -> {}", r.method, r.url, r.status))
        .rev()
        .take(KEEP_RECENT)
        .collect();

    let ws_messages: Vec<String> = raw.ws_messages.into_iter().rev().take(KEEP_RECENT).collect();

    TelemetrySnapshot {
        current_url: raw.current_url,
        console_errors,
        network_4xx_5xx,
        ws_messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchyn_core::{ConsoleLog, NetworkRequest};

    #[test]
    fn keeps_only_errors_and_warns() {
        let raw = RawTelemetry {
            console_logs: vec![
                ConsoleLog { level: "info".into(), message: "boring".into() },
                ConsoleLog { level: "error".into(), message: "boom".into() },
            ],
            ..Default::default()
        };
        let snap = compress_telemetry(raw);
        assert_eq!(snap.console_errors.len(), 1);
        assert!(snap.console_errors[0].contains("boom"));
    }

    #[test]
    fn keeps_only_4xx_5xx_requests() {
        let raw = RawTelemetry {
            network_requests: vec![
                NetworkRequest { method: "GET".into(), url: "/a".into(), status: 200 },
                NetworkRequest { method: "POST".into(), url: "/b".into(), status: 500 },
            ],
            ..Default::default()
        };
        let snap = compress_telemetry(raw);
        assert_eq!(snap.network_4xx_5xx.len(), 1);
        assert!(snap.network_4xx_5xx[0].contains("500"));
    }
}
