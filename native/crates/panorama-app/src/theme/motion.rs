//! Shared CSS motion tokens.
use std::time::Duration;

/// CSS fast motion duration.
pub const DURATION_FAST: Duration = Duration::from_millis(160);
/// CSS standard motion duration.
pub const DURATION_STANDARD: Duration = Duration::from_millis(220);
/// CSS deliberate motion duration.
pub const DURATION_DELIBERATE: Duration = Duration::from_millis(320);
/// Original CSS editorial Bezier control points.
pub const EASE_EDITORIAL: [f64; 4] = [0.22, 1.0, 0.36, 1.0];

/// The CSS shimmer animation's typed configuration.
#[derive(Clone, Copy, Debug)]
pub struct AnimationToken {
    /// Animation cycle duration.
    pub duration: Duration,
    /// Cubic Bezier timing coordinates.
    pub easing: [f64; 4],
    /// Whether the cycle repeats indefinitely.
    pub infinite: bool,
}

/// CSS shimmer 1.6s ease-in-out infinite.
pub const SHIMMER: AnimationToken = AnimationToken {
    duration: Duration::from_millis(1600),
    easing: [0.42, 0.0, 0.58, 1.0],
    infinite: true,
};
