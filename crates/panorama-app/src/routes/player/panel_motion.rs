//! Animation of the shared panel surface: rise, switch and sink.
use super::panel::{Change, PanelId};
use crate::{
    theme::motion::{EASE_DEFAULT, EASE_EDITORIAL},
    transition::Tween,
};
use gpui::{Pixels, Size, px};
use std::time::{Duration, Instant};

const OPEN: Duration = Duration::from_millis(220);
const CLOSE: Duration = Duration::from_millis(150);
const SWITCH: Duration = Duration::from_millis(250);
const CLOSE_EASE: [f64; 4] = [0.23, 1.0, 0.32, 1.0];
/// The surface starts this far below its resting place.
const RISE: f32 = 6.0;
/// Panel bottom edge above the icon pill, and the fallback distance from the window bottom.
pub const PILL_GAP: f32 = 8.0;
pub const FALLBACK_BOTTOM: f32 = 96.0;

/// One panel switch in flight. The incoming page is measured before the surface resizes.
#[derive(Clone, Copy, Debug)]
pub struct Switch {
    pub from: PanelId,
    pub direction: i8,
    pub from_size: Size<Pixels>,
    to_size: Option<Size<Pixels>>,
}

/// Position of the two pages and the surface while switching.
#[derive(Clone, Copy, Debug)]
pub struct SwitchPose {
    /// 0 at the start of the slide, 1 at its end.
    pub t: f32,
    pub size: Size<Pixels>,
}

/// Motion state of the panel surface.
pub struct PanelMotion {
    appear: Tween,
    slide: Tween,
    switch: Option<Switch>,
    /// Surface bottom edge, measured when it opened.
    pub bottom: f32,
}

impl Default for PanelMotion {
    fn default() -> Self {
        Self {
            appear: Tween::fixed(0.0),
            slide: Tween::fixed(0.0),
            switch: None,
            bottom: FALLBACK_BOTTOM,
        }
    }
}

impl PanelMotion {
    /// Apply a state change at `now`; `from_size` is the surface's current size.
    pub fn apply(&mut self, change: Change, from_size: Option<Size<Pixels>>, now: Instant) {
        match change {
            Change::Opened => {
                self.appear.retarget(1.0, OPEN, Some(EASE_EDITORIAL), now);
                self.switch = None;
            }
            Change::Closed => {
                self.appear.retarget(0.0, CLOSE, Some(CLOSE_EASE), now);
                self.switch = None;
            }
            Change::Switched { from, direction } => {
                self.switch = from_size.map(|from_size| Switch {
                    from,
                    direction,
                    from_size,
                    to_size: None,
                });
                self.slide = Tween::fixed(0.0);
            }
        }
    }

    /// The switch in flight, if any.
    pub fn switching(&self) -> Option<Switch> {
        self.switch
    }

    /// Advance a switch: start the slide once the incoming page is measured, end it when done.
    pub fn tick(&mut self, incoming: Option<Size<Pixels>>, now: Instant, reduced: bool) {
        let Some(switch) = &mut self.switch else {
            return;
        };
        if switch.to_size.is_none()
            && let Some(size) = incoming
        {
            switch.to_size = Some(size);
            self.slide.retarget(1.0, SWITCH, Some(EASE_DEFAULT), now);
        }
        if switch.to_size.is_some() && (reduced || !self.slide.moving(now, false)) {
            self.switch = None;
        }
    }

    /// Opacity of the surface; 0 means it can unmount once closed.
    pub fn appear(&self, now: Instant, reduced: bool) -> f32 {
        self.appear.value(now, reduced)
    }

    /// Vertical offset while rising or sinking.
    pub fn offset(&self, now: Instant, reduced: bool) -> f32 {
        (1.0 - self.appear(now, reduced)) * RISE
    }

    /// Where the slide and the surface size are.
    pub fn pose(&self, now: Instant, reduced: bool) -> Option<SwitchPose> {
        let switch = self.switch?;
        let t = self.slide.value(now, reduced);
        let size = match switch.to_size {
            Some(to) => Size {
                width: lerp(switch.from_size.width, to.width, t),
                height: lerp(switch.from_size.height, to.height, t),
            },
            None => switch.from_size,
        };
        Some(SwitchPose { t, size })
    }

    /// Whether another frame is needed.
    pub fn moving(&self, now: Instant, reduced: bool) -> bool {
        !reduced
            && (self.appear.moving(now, false)
                || self.slide.moving(now, false)
                || self.switch.is_some())
    }
}

fn lerp(from: Pixels, to: Pixels, t: f32) -> Pixels {
    px(f32::from(from) + (f32::from(to) - f32::from(from)) * t)
}

/// Horizontal offset (as a fraction of the page width) and opacity of the leaving page.
pub fn outgoing(direction: i8, t: f32) -> (f32, f32) {
    (-f32::from(direction) * t, 1.0 - t)
}

/// Horizontal offset and opacity of the arriving page.
pub fn incoming(direction: i8, t: f32) -> (f32, f32) {
    (f32::from(direction) * (1.0 - t), t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: f32, height: f32) -> Size<Pixels> {
        Size {
            width: px(width),
            height: px(height),
        }
    }

    #[test]
    fn pages_slide_a_full_width_with_a_cross_fade() {
        assert_eq!(outgoing(1, 0.0), (0.0, 1.0));
        assert_eq!(outgoing(1, 1.0), (-1.0, 0.0));
        assert_eq!(incoming(1, 0.0), (1.0, 0.0));
        assert_eq!(incoming(1, 1.0), (0.0, 1.0));
        assert_eq!(incoming(-1, 0.0), (-1.0, 0.0));
        assert_eq!(outgoing(-1, 1.0), (1.0, 0.0));
    }

    #[test]
    fn a_switch_waits_for_the_measurement_then_resizes_and_ends() {
        let now = Instant::now();
        let mut motion = PanelMotion::default();
        motion.apply(
            Change::Switched {
                from: PanelId::Audio,
                direction: 1,
            },
            Some(size(288.0, 120.0)),
            now,
        );
        motion.tick(None, now, false);
        let held = motion.pose(now, false).expect("held pose");
        assert_eq!((held.t, held.size), (0.0, size(288.0, 120.0)));
        motion.tick(Some(size(416.0, 300.0)), now, false);
        assert!(motion.switching().is_some());
        let later = now + SWITCH + Duration::from_millis(1);
        let end = motion.pose(later, false).expect("end pose");
        assert_eq!((end.t, end.size), (1.0, size(416.0, 300.0)));
        motion.tick(None, later, false);
        assert!(motion.switching().is_none());
    }

    #[test]
    fn reduced_motion_finishes_a_switch_at_once() {
        let now = Instant::now();
        let mut motion = PanelMotion::default();
        motion.apply(
            Change::Switched {
                from: PanelId::Audio,
                direction: -1,
            },
            Some(size(288.0, 120.0)),
            now,
        );
        motion.tick(Some(size(416.0, 300.0)), now, true);
        assert!(motion.switching().is_none());
        assert!(!motion.moving(now, true));
    }

    #[test]
    fn reopening_mid_close_continues_from_the_current_opacity() {
        let now = Instant::now();
        let mut motion = PanelMotion::default();
        motion.apply(Change::Opened, None, now);
        let open = now + OPEN + Duration::from_millis(1);
        motion.apply(Change::Closed, None, open);
        let mid = open + CLOSE / 2;
        let before = motion.appear(mid, false);
        assert!(before > 0.0 && before < 1.0);
        motion.apply(Change::Opened, None, mid);
        assert!((motion.appear(mid, false) - before).abs() < 1e-4);
    }

    #[test]
    fn opening_rises_and_closing_sinks() {
        let now = Instant::now();
        let mut motion = PanelMotion::default();
        motion.apply(Change::Opened, None, now);
        assert_eq!(motion.appear(now, false), 0.0);
        assert_eq!(motion.offset(now, false), RISE);
        let open = now + OPEN + Duration::from_millis(1);
        assert_eq!(motion.appear(open, false), 1.0);
        assert_eq!(motion.offset(open, false), 0.0);
        motion.apply(Change::Closed, None, open);
        let closed = open + CLOSE + Duration::from_millis(1);
        assert_eq!(motion.appear(closed, false), 0.0);
        assert!(!motion.moving(closed, false));
    }
}
