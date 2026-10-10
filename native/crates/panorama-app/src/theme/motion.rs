//! Shared CSS motion tokens.
use std::time::Duration;

/// CSS fast motion duration.
pub const DURATION_FAST: Duration = Duration::from_millis(160);
/// CSS standard motion duration.
pub const DURATION_STANDARD: Duration = Duration::from_millis(220);
/// CSS deliberate motion duration.
pub const DURATION_DELIBERATE: Duration = Duration::from_millis(320);
/// Header color and pressed-trigger transition.
pub const HEADER_COLOR: Duration = Duration::from_millis(150);
/// Sticky header translation.
pub const HEADER_SLIDE: Duration = Duration::from_millis(200);
/// Account popup entrance.
pub const MENU_ENTER: Duration = Duration::from_millis(180);
/// Account popup exit.
pub const MENU_EXIT: Duration = Duration::from_millis(150);
/// CSS ease-in-out.
pub const EASE_IN_OUT: [f64; 4] = [0.42, 0.0, 0.58, 1.0];
/// CSS ease-in.
pub const EASE_IN: [f64; 4] = [0.42, 0.0, 1.0, 1.0];
/// CSS ease-out.
pub const EASE_OUT: [f64; 4] = [0.0, 0.0, 0.58, 1.0];
/// Account popup entrance curve.
pub const EASE_MENU: [f64; 4] = [0.2, 0.0, 0.0, 1.0];
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

/// Tailwind's default transition easing.
pub const EASE_DEFAULT: [f64; 4] = [0.4, 0.0, 0.2, 1.0];
