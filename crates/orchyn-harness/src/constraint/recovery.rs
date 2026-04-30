use image::GenericImageView;
use orchyn_core::{FailPolicy, ManifestStep, OrchynError, Result};

/// Detects when an actor is stuck by tracking perceptual hashes of recent
/// screenshots. If the same scene reappears `repeat_threshold` times within
/// the rolling window, [`is_stuck`] returns true.
pub struct StuckDetector {
    history: Vec<u64>,
    distance_threshold: u32,
    repeat_threshold: usize,
    window: usize,
}

impl StuckDetector {
    pub fn new(distance_threshold: u32) -> Self {
        Self {
            history: Vec::new(),
            distance_threshold,
            repeat_threshold: 2,
            window: 6,
        }
    }

    /// Returns true if the given screenshot matches a recent one closely
    /// enough that we suspect the actor is looping or wedged.
    pub fn is_stuck(&mut self, screenshot_png: &[u8]) -> Result<bool> {
        let hash = compute_dhash(screenshot_png)?;
        let near_matches = self
            .history
            .iter()
            .filter(|&&h| hamming(hash, h) <= self.distance_threshold)
            .count();
        let stuck = near_matches >= self.repeat_threshold;
        if self.history.len() >= self.window {
            self.history.remove(0);
        }
        self.history.push(hash);
        Ok(stuck)
    }

    pub fn reset(&mut self) {
        self.history.clear();
    }
}

fn compute_dhash(png: &[u8]) -> Result<u64> {
    let img = image::load_from_memory(png)
        .map_err(|e| OrchynError::Internal(format!("dhash decode: {e}")))?;
    let small = img
        .grayscale()
        .resize_exact(9, 8, image::imageops::FilterType::Lanczos3);
    let luma = small.to_luma8();
    let bytes = luma.as_raw();
    let mut hash: u64 = 0;
    let mut bit = 0;
    for row in 0..8usize {
        for col in 0..8usize {
            let left = bytes[row * 9 + col];
            let right = bytes[row * 9 + col + 1];
            if left > right {
                hash |= 1u64 << bit;
            }
            bit += 1;
        }
    }
    let _ = small.dimensions();
    Ok(hash)
}

fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Decides what to do when a step fails: retry, skip, or halt the run.
pub struct RecoveryPolicy {
    pub max_retries: usize,
    pub max_waits: usize,
}

impl RecoveryPolicy {
    pub fn new(max_retries: usize, max_waits: usize) -> Self {
        Self { max_retries, max_waits }
    }

    pub fn decide(&self, step: &ManifestStep, attempt: usize, wait_count: usize) -> RecoveryAction {
        if attempt >= self.max_retries || wait_count >= self.max_waits {
            return RecoveryAction::Halt;
        }
        match step.on_fail {
            FailPolicy::Halt => RecoveryAction::Halt,
            FailPolicy::Skip => RecoveryAction::Skip,
            FailPolicy::Retry => RecoveryAction::Retry,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    Retry,
    Skip,
    Halt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn make_png(w: u32, h: u32, fill: [u8; 4]) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, Rgba(fill));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn stuck_after_repeated_identical_frames() {
        let mut det = StuckDetector::new(3);
        let frame = make_png(32, 32, [128, 128, 128, 255]);
        assert!(!det.is_stuck(&frame).unwrap());
        assert!(!det.is_stuck(&frame).unwrap());
        // The third identical frame should trip the detector (>= 2 near matches in history).
        assert!(det.is_stuck(&frame).unwrap());
    }

    #[test]
    fn policy_halts_on_halt_step_or_exhausted_retries() {
        let policy = RecoveryPolicy::new(3, 3);
        let halt_step = ManifestStep {
            step: 0,
            actor: "x".into(),
            instruction: "".into(),
            on_fail: FailPolicy::Halt,
            action_hint: None,
        };
        assert_eq!(policy.decide(&halt_step, 0, 0), RecoveryAction::Halt);

        let retry_step = ManifestStep {
            on_fail: FailPolicy::Retry,
            ..halt_step.clone()
        };
        assert_eq!(policy.decide(&retry_step, 0, 0), RecoveryAction::Retry);
        assert_eq!(policy.decide(&retry_step, 3, 0), RecoveryAction::Halt);
    }
}
