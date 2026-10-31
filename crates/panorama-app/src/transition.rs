use std::time::{Duration, Instant};

/// Sample a CSS cubic Bezier, solving its time axis.
pub fn bezier(time: f32, points: [f64; 4]) -> f32 {
    let time = f64::from(time.clamp(0.0, 1.0));
    let [x1, y1, x2, y2] = points;
    let curve = |u: f64, a: f64, b: f64| {
        3.0 * (1.0 - u).powi(2) * u * a + 3.0 * (1.0 - u) * u * u * b + u.powi(3)
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if curve(mid, x1, x2) < time {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    if time <= 0.0 {
        0.0
    } else if time >= 1.0 {
        1.0
    } else {
        curve((lo + hi) * 0.5, y1, y2) as f32
    }
}

/// Reversible scalar motion; reduced motion samples the target immediately.
#[derive(Clone, Copy)]
pub struct Tween {
    from: f32,
    /// Current destination.
    pub target: f32,
    start: Instant,
    duration: Duration,
    easing: Option<[f64; 4]>,
}
impl Tween {
    /// Start already settled.
    pub fn fixed(value: f32) -> Self {
        Self {
            from: value,
            target: value,
            start: Instant::now(),
            duration: Duration::ZERO,
            easing: None,
        }
    }
    /// Current value, respecting the explicit reduced-motion policy.
    pub fn value(self, now: Instant, reduced: bool) -> f32 {
        if reduced || self.duration.is_zero() {
            return self.target;
        }
        let t = (now.saturating_duration_since(self.start).as_secs_f32()
            / self.duration.as_secs_f32())
        .clamp(0.0, 1.0);
        let t = self.easing.map_or(t, |points| bezier(t, points));
        self.from + (self.target - self.from) * t
    }
    /// Retarget from the currently displayed value without a jump.
    pub fn retarget(
        &mut self,
        target: f32,
        duration: Duration,
        easing: Option<[f64; 4]>,
        now: Instant,
    ) {
        if self.target == target {
            return;
        }
        self.from = self.value(now, false);
        self.target = target;
        self.start = now;
        self.duration = duration;
        self.easing = easing;
    }
    /// Whether further frames can change this value.
    pub fn moving(self, now: Instant, reduced: bool) -> bool {
        !reduced
            && self.from != self.target
            && now.saturating_duration_since(self.start) < self.duration
    }
}
