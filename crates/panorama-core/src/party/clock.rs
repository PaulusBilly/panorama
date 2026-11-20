use std::collections::VecDeque;

const SAMPLES: usize = 8;
/// A longer silence means the machine slept or the network moved, so old samples lie.
const STALE_GAP_MS: i64 = 5_000;

/// NTP-style estimate of the host's clock from ping/pong round trips. The sample with the
/// lowest round trip among the last eight wins, since queueing delay only ever adds error.
#[derive(Clone, Debug, Default)]
pub struct ClockSync {
    samples: VecDeque<Sample>,
    last_local_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    rtt_ms: i64,
    offset_ms: i64,
}

impl ClockSync {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, sent_ms: i64, host_ms: i64, recv_ms: i64) {
        let rtt_ms = recv_ms.saturating_sub(sent_ms);
        if rtt_ms < 0 {
            return;
        }
        if self.samples.len() == SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample {
            rtt_ms,
            offset_ms: midpoint(host_ms, sent_ms, recv_ms),
        });
    }

    /// Host time is local time plus this.
    pub fn offset_ms(&self) -> Option<i64> {
        self.samples
            .iter()
            .min_by_key(|sample| sample.rtt_ms)
            .map(|sample| sample.offset_ms)
    }

    pub fn host_now(&self, local_ms: i64) -> Option<i64> {
        self.offset_ms()
            .map(|offset| local_ms.saturating_add(offset))
    }

    /// Returns whether the samples were dropped because of a gap since the last call.
    pub fn note_local_time(&mut self, local_ms: i64) -> bool {
        let stale = self
            .last_local_ms
            .is_some_and(|last| local_ms.saturating_sub(last) > STALE_GAP_MS);
        if stale {
            self.samples.clear();
        }
        self.last_local_ms = Some(local_ms);
        stale
    }
}

/// `((host - sent) + (host - recv)) / 2`, widened so no input overflows it.
fn midpoint(host_ms: i64, sent_ms: i64, recv_ms: i64) -> i64 {
    let (host, sent, recv) = (
        i128::from(host_ms),
        i128::from(sent_ms),
        i128::from(recv_ms),
    );
    let offset = ((host - sent) + (host - recv)) / 2;
    offset.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}
