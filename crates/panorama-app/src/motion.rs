//! Retained presence for interruptible enter and exit motion.
use std::time::Instant;

pub use crate::theme::{DURATION_DELIBERATE, DURATION_FAST, DURATION_STANDARD, EASE_EDITORIAL};

/// Solve x(u)=time for CSS's editorial cubic Bezier, then return y(u).
pub fn ease_editorial(time: f32) -> f32 {
    let time = f64::from(time.clamp(0.0, 1.0));
    let [x1, y1, x2, y2] = EASE_EDITORIAL;
    let curve = |u: f64, a: f64, b: f64| {
        3.0 * (1.0 - u).powi(2) * u * a + 3.0 * (1.0 - u) * u * u * b + u.powi(3)
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..40 {
        let middle = (low + high) * 0.5;
        if curve(middle, x1, x2) < time {
            low = middle;
        } else {
            high = middle;
        }
    }
    if time == 0.0 {
        0.0
    } else if time == 1.0 {
        1.0
    } else {
        curve((low + high) * 0.5, y1, y2) as f32
    }
}

/// Sampled opacity and vertical displacement.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    /// Element opacity.
    pub opacity: f32,
    /// Logical vertical pixel offset.
    pub y: f32,
}

/// A retained view and its independent motion timeline.
pub struct Presence<T> {
    /// The live view retained until exit completes.
    pub view: T,
    /// Whether the view is leaving.
    pub exiting: bool,
    from: Pose,
    start: Instant,
}

impl<T> Presence<T> {
    /// Begin an incoming view.
    pub fn enter(view: T, now: Instant) -> Self {
        Self {
            view,
            exiting: false,
            from: Pose {
                opacity: 0.0,
                y: 16.0,
            },
            start: now,
        }
    }
    /// Sample this timeline without changing its state.
    pub fn pose(&self, now: Instant) -> Pose {
        let duration = if self.exiting {
            DURATION_FAST
        } else {
            DURATION_DELIBERATE
        };
        let time = now.saturating_duration_since(self.start).as_secs_f32() / duration.as_secs_f32();
        let progress = ease_editorial(time);
        let target = if self.exiting { 0.0 } else { 1.0 };
        Pose {
            opacity: self.from.opacity + (target - self.from.opacity) * progress,
            y: if self.exiting {
                self.from.y
            } else {
                self.from.y * (1.0 - progress)
            },
        }
    }
    /// Begin exit from the current pose so interruption never jumps.
    pub fn exit(&mut self, now: Instant) {
        if !self.exiting {
            self.from = self.pose(now);
            self.start = now;
            self.exiting = true;
        }
    }
    /// Resume a still-retained view from its current pose.
    pub fn resume(&mut self, now: Instant) {
        self.from = self.pose(now);
        self.start = now;
        self.exiting = false;
    }
    /// Whether this timeline has reached its target.
    pub fn settled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start)
            >= if self.exiting {
                DURATION_FAST
            } else {
                DURATION_DELIBERATE
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn bezier_references_and_monotonicity() {
        assert_eq!(ease_editorial(0.0), 0.0);
        assert_eq!(ease_editorial(1.0), 1.0);
        assert!((ease_editorial(0.5) - 0.9613825).abs() < 0.00001);
        for i in 0..1000 {
            assert!(ease_editorial(i as f32 / 1000.0) <= ease_editorial((i + 1) as f32 / 1000.0));
        }
    }
    #[test]
    fn interruption_is_continuous() {
        let start = Instant::now();
        let now = start + Duration::from_millis(70);
        let mut presence = Presence::enter((), start);
        let pose = presence.pose(now);
        presence.exit(now);
        assert_eq!(presence.pose(now).opacity, pose.opacity);
        assert_eq!(presence.pose(now).y, pose.y);
        let now = now + Duration::from_millis(30);
        let pose = presence.pose(now);
        presence.resume(now);
        assert_eq!(presence.pose(now).opacity, pose.opacity);
        assert_eq!(presence.pose(now).y, pose.y);
        assert!(presence.settled(now + DURATION_DELIBERATE));
    }
}
