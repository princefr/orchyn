use orchyn_core::action::{Action, AnnotatedElement, LlmDecision};
use orchyn_core::{OrchynError, Result};

/// Parses the model's raw JSON into a [`LlmDecision`] and rejects responses
/// that would target unknown elements or stray off the active actor / step.
pub fn parse_and_validate(
    raw: &str,
    element_map: &[AnnotatedElement],
    active_actor: &str,
    expected_step: usize,
) -> Result<LlmDecision> {
    let trimmed = extract_json_object(raw)?;
    let decision: LlmDecision = serde_json::from_str(&trimmed)
        .map_err(|e| OrchynError::Validation(format!("invalid decision JSON: {e}")))?;

    if decision.actor_id != active_actor {
        return Err(OrchynError::Validation(format!(
            "actor mismatch: expected {active_actor}, got {}",
            decision.actor_id
        )));
    }
    if decision.manifest_step != expected_step {
        return Err(OrchynError::Validation(format!(
            "step mismatch: expected {expected_step}, got {}",
            decision.manifest_step
        )));
    }

    let valid_target = |t: u8| t == 0 || element_map.iter().any(|e| e.id == t);
    let bad_target = |t: u8| {
        Err::<(), OrchynError>(OrchynError::Validation(format!(
            "invalid target id: {t}"
        )))
    };

    match &decision.action {
        Action::Click { target } | Action::Type { target, .. } | Action::Clear { target } => {
            if !valid_target(*target) {
                bad_target(*target)?;
            }
        }
        Action::Drag { target, destination } => {
            if !valid_target(*target) {
                bad_target(*target)?;
            }
            if !valid_target(*destination) {
                bad_target(*destination)?;
            }
        }
        _ => {}
    }

    Ok(decision)
}

/// Pulls the first balanced JSON object out of free-form model output. Most
/// 0.8B responses are clean JSON, but smaller models occasionally wrap them
/// in markdown fences or chatter.
fn extract_json_object(raw: &str) -> Result<String> {
    let bytes = raw.as_bytes();
    let start = raw
        .find('{')
        .ok_or_else(|| OrchynError::Validation("no JSON object in response".into()))?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(raw[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    Err(OrchynError::Validation("unbalanced braces in response".into()))
}

/// Calls `op` with retries and exponential-ish backoff. Returns the first
/// success, or `OrchynError::RecoveryExhausted` after `max_retries`.
pub async fn execute_with_retry<F, Fut, T>(mut op: F, max_retries: usize) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_err: Option<OrchynError> = None;
    for attempt in 0..max_retries.max(1) {
        match op().await {
            Ok(value) => return Ok(value),
            Err(e) => {
                last_err = Some(e);
                let delay = std::time::Duration::from_millis(500 * (attempt as u64 + 1));
                tokio::time::sleep(delay).await;
            }
        }
    }
    Err(OrchynError::RecoveryExhausted(format!(
        "failed after {max_retries} retries: {:?}",
        last_err
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchyn_core::action::BoundingBox;

    fn elem(id: u8) -> AnnotatedElement {
        AnnotatedElement {
            id,
            element_type: "button".into(),
            text: None,
            bounds: BoundingBox { x: 0, y: 0, width: 1, height: 1 },
        }
    }

    #[test]
    fn rejects_unknown_target() {
        let raw = r#"{"actor_id":"alice","manifest_step":0,"thought":"x","action":"CLICK","target":42}"#;
        let err = parse_and_validate(raw, &[elem(1)], "alice", 0).unwrap_err();
        assert!(matches!(err, OrchynError::Validation(_)));
    }

    #[test]
    fn accepts_valid_click() {
        let raw = r#"{"actor_id":"alice","manifest_step":0,"thought":"x","action":"CLICK","target":1}"#;
        let d = parse_and_validate(raw, &[elem(1)], "alice", 0).unwrap();
        assert!(matches!(d.action, Action::Click { target: 1 }));
    }

    #[test]
    fn extracts_json_from_markdown_fence() {
        let raw = "```json\n{\"actor_id\":\"a\",\"manifest_step\":0,\"thought\":\"\",\"action\":\"WAIT\",\"value\":\"loading\"}\n```";
        let d = parse_and_validate(raw, &[], "a", 0).unwrap();
        assert!(matches!(d.action, Action::Wait { .. }));
    }

    #[tokio::test]
    async fn retry_returns_first_success() {
        use std::cell::RefCell;
        let attempts = RefCell::new(0usize);
        let result: Result<u32> = execute_with_retry(
            || {
                let mut a = attempts.borrow_mut();
                *a += 1;
                let count = *a;
                async move {
                    if count < 2 {
                        Err(OrchynError::Internal("flaky".into()))
                    } else {
                        Ok(7)
                    }
                }
            },
            3,
        )
        .await;
        assert_eq!(result.unwrap(), 7);
    }
}
