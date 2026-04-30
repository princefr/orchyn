use orchyn_core::ManifestStep;

/// Drives manifest execution: hands out (actor, step_idx) tuples and tracks
/// retry/skip/advance decisions made by the recovery policy.
pub struct TurnScheduler {
    manifest: Vec<ManifestStep>,
    current_step: usize,
}

impl TurnScheduler {
    pub fn new(manifest: Vec<ManifestStep>) -> Self {
        Self { manifest, current_step: 0 }
    }

    pub fn next_turn(&self) -> Option<(String, usize)> {
        let step = self.manifest.get(self.current_step)?;
        Some((step.actor.clone(), self.current_step))
    }

    pub fn advance(&mut self) {
        self.current_step += 1;
    }

    pub fn retry_current(&mut self) {
        // Stay on the current step intentionally.
    }

    pub fn skip_current(&mut self) {
        self.current_step += 1;
    }

    pub fn is_complete(&self) -> bool {
        self.current_step >= self.manifest.len()
    }

    pub fn current_step(&self) -> Option<&ManifestStep> {
        self.manifest.get(self.current_step)
    }

    pub fn manifest(&self) -> &[ManifestStep] {
        &self.manifest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchyn_core::FailPolicy;

    fn step(idx: usize, actor: &str) -> ManifestStep {
        ManifestStep {
            step: idx,
            actor: actor.to_string(),
            instruction: "noop".to_string(),
            on_fail: FailPolicy::Halt,
            action_hint: None,
        }
    }

    #[test]
    fn next_then_advance_walks_manifest() {
        let mut s = TurnScheduler::new(vec![step(0, "alice"), step(1, "bob")]);
        assert_eq!(s.next_turn(), Some(("alice".into(), 0)));
        s.advance();
        assert_eq!(s.next_turn(), Some(("bob".into(), 1)));
        s.advance();
        assert!(s.is_complete());
        assert_eq!(s.next_turn(), None);
    }

    #[test]
    fn retry_keeps_position() {
        let mut s = TurnScheduler::new(vec![step(0, "alice")]);
        s.retry_current();
        assert_eq!(s.next_turn(), Some(("alice".into(), 0)));
    }
}
