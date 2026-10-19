//! Sanitized diagnostics ported from `desktop/main/media-proxy.ts`.
use crate::media::MediaError;
use std::sync::Arc;

/// Outcome of the initial range probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeResult {
    /// Source supports validated byte ranges.
    Ranged,
    /// Source requires direct streaming.
    Passthrough,
    /// Probe failed.
    Failed,
}

/// Transport events that cannot include a source URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaProxyEvent {
    /// Initial source probe completed.
    Probe {
        /// Probe mode or failure.
        result: ProbeResult,
        /// Upstream response status, when known.
        status: Option<u16>,
        /// Elapsed monotonic milliseconds.
        ms: u64,
        /// Sanitized failure, if any.
        error: Option<MediaError>,
    },
    /// One range attempt is being retried.
    RangeRetry {
        /// Zero-based chunk index.
        index: u64,
        /// Zero-based attempt number.
        attempt: u32,
        /// Upstream status, when known.
        status: Option<u16>,
        /// Whether inactivity triggered cancellation.
        stalled: bool,
        /// Current parallelism cap.
        parallel: usize,
    },
    /// Warm-up completed probing.
    Warm {
        /// True when source is ready.
        ready: bool,
        /// Sanitized failure, if any.
        error: Option<MediaError>,
    },
}

/// Optional host callback for structured, URL-free diagnostics.
pub type EventSink = Arc<dyn Fn(MediaProxyEvent) + Send + Sync>;
