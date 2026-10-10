use std::time::Instant;

/// Frame-completion samples, with all reporting deferred through the logger.
pub struct Samples {
    /// Start of the five-second run.
    pub start: Instant,
    last: Instant,
    intervals: Vec<f64>,
}
impl Samples {
    /// Begin a run after startup and the hero fade have settled.
    pub fn new(now: Instant) -> Self {
        Self {
            start: now,
            last: now,
            intervals: vec![],
        }
    }
    /// Record completion of an actual GPUI rendered frame.
    pub fn frame(&mut self, now: Instant) {
        if now > self.last {
            self.intervals
                .push(now.duration_since(self.last).as_secs_f64());
            self.last = now;
        }
    }
    /// Gates-ui's average and reciprocal mean of the slowest ceil(1%) intervals.
    pub fn report(&self, now: Instant) -> String {
        let elapsed = now.duration_since(self.start).as_secs_f64();
        let mut sorted = self.intervals.clone();
        sorted.sort_by(f64::total_cmp);
        let count = sorted.len().div_ceil(100).max(1);
        let slow = sorted.iter().rev().take(count).sum::<f64>() / count as f64;
        format!(
            "Home fixture scroll: cards=200 frames={} elapsed_s={elapsed:.3} average_fps={:.3} low_1pct_fps={:.3} worst_ms={:.3}",
            sorted.len(),
            sorted.len() as f64 / elapsed,
            1.0 / slow,
            sorted.last().copied().unwrap_or_default() * 1000.0
        )
    }
}
