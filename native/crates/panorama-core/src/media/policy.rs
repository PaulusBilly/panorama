//! Measured buffering policy ported from `desktop/main/buffering-policy.ts`.

/// Player and transport measurements for one policy update.
#[derive(Clone, Debug)]
pub struct BufferingSample {
    /// Monotonic milliseconds.
    pub now_ms: u64,
    /// Player's forward buffer, if known.
    pub buffered_seconds: Option<f64>,
    /// Media bitrate in megabits per second.
    pub source_mbps: Option<f64>,
    /// Recent measured download rate.
    pub download_mbps: Option<f64>,
    /// True when active transfer is demanded by a reader.
    pub transfer_demanded: bool,
    /// Player is waiting for buffered media.
    pub buffering: bool,
    /// Playback is paused.
    pub paused: bool,
    /// Current playback speed multiplier.
    pub playback_speed: f64,
    /// Upstream is currently in cooldown.
    pub throttled: bool,
}

/// Connection cap and player buffer thresholds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BufferingDecision {
    /// Requested parallelism, between one and six.
    pub parallel: usize,
    /// Forward buffer target, between 60 and 120 seconds.
    pub target_ahead_seconds: u32,
    /// Initial playback threshold.
    pub initial_buffer_seconds: u32,
    /// Rebuffer resume threshold.
    pub resume_buffer_seconds: u32,
}

struct Probe {
    previous: usize,
    rate: f64,
    until: u64,
}

/// Stateful, synchronous throughput probing policy.
pub struct BufferingPolicy {
    parallel: usize,
    target: u32,
    changed_at: u64,
    active_since: Option<u64>,
    rates: std::collections::VecDeque<f64>,
    probe: Option<Probe>,
    was_buffering: bool,
}

impl Default for BufferingPolicy {
    fn default() -> Self {
        Self {
            parallel: 3,
            target: 60,
            changed_at: 0,
            active_since: None,
            rates: Default::default(),
            probe: None,
            was_buffering: false,
        }
    }
}

impl BufferingPolicy {
    /// Creates the initial three-connection, 60-second policy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one measurement with the original probe and cooldown rules.
    pub fn update(&mut self, sample: &BufferingSample) -> BufferingDecision {
        let demand = sample
            .source_mbps
            .map(|rate| rate * sample.playback_speed.max(0.25));
        if sample.throttled {
            self.parallel = self.parallel.saturating_sub(1).max(1);
            self.changed_at = sample.now_ms;
            self.probe = None;
            self.active_since = None;
            self.rates.clear();
        } else if !sample.transfer_demanded || sample.paused {
            if let Some(probe) = &self.probe {
                self.parallel = probe.previous;
                self.changed_at = sample.now_ms;
            }
            self.active_since = None;
            self.rates.clear();
            self.probe = None;
        } else {
            let active_since = *self.active_since.get_or_insert(sample.now_ms);
            if let Some(rate) = sample.download_mbps.filter(|rate| rate.is_finite()) {
                self.rates.push_back(rate);
                if self.rates.len() > 50 {
                    self.rates.pop_front();
                }
            }
            let rate = if self.rates.is_empty() {
                0.0
            } else {
                self.rates.iter().sum::<f64>() / self.rates.len() as f64
            };
            if sample.buffering
                && !self.was_buffering
                && demand.is_some_and(|demand| rate > demand * 1.1)
            {
                self.target = (self.target + 15).min(120);
            }
            if self
                .probe
                .as_ref()
                .is_some_and(|probe| sample.now_ms >= probe.until)
            {
                if let Some(probe) = self.probe.take()
                    && rate < probe.rate * 1.1
                {
                    self.parallel = probe.previous;
                }
                self.changed_at = sample.now_ms;
            } else if self.probe.is_none()
                && sample.now_ms.saturating_sub(self.changed_at) >= 10_000
                && sample.now_ms.saturating_sub(active_since) >= 5000
                && self.rates.len() >= 5
            {
                if sample
                    .buffered_seconds
                    .is_some_and(|buffer| buffer >= f64::from(self.target + 15))
                    && demand.is_some_and(|demand| rate >= demand * 1.2)
                {
                    self.parallel = 1;
                    self.changed_at = sample.now_ms;
                } else if sample.buffered_seconds.unwrap_or(0.0) < f64::from(self.target) - 15.0
                    && self.parallel < 6
                {
                    self.probe = Some(Probe {
                        previous: self.parallel,
                        rate,
                        until: sample.now_ms + 5000,
                    });
                    self.parallel += 1;
                    self.rates.clear();
                }
            }
        }
        self.was_buffering = sample.buffering;
        BufferingDecision {
            parallel: self.parallel,
            target_ahead_seconds: self.target,
            initial_buffer_seconds: 5,
            resume_buffer_seconds: 5,
        }
    }
}

#[cfg(test)]
mod tests;
