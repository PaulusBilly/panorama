//! A sideways row that comes to rest on an item, in the manner of Apple TV: scroll freely, and
//! once the gesture stops the row glides so an item sits exactly on the column's left edge.
//! Shared by the home Continue row and the film page's Cast & Crew and Awards & Festivals.
use crate::{
    theme::{DURATION_DELIBERATE, EASE_EDITORIAL},
    transition::Tween,
};
use gpui::{ScrollHandle, point, px};
use std::time::{Duration, Instant};

/// Quiet time after the last wheel event, momentum included, before the row settles.
const SETTLE_AFTER: Duration = Duration::from_millis(140);

#[derive(Default)]
pub struct Carousel {
    pub scroll: ScrollHandle,
    /// When the pointer last scrolled the row; the row settles once it has been quiet.
    wheeled: Option<Instant>,
    /// The glide onto an item, in scroll pixels from the start of the row.
    glide: Option<Tween>,
    /// The item keyboard focus last brought onto the column.
    focused: Option<usize>,
}

/// Where a row left at `position` comes to rest: the nearest item edge, `pitch` apart.
fn rest(position: f32, pitch: f32) -> f32 {
    (position / pitch).round() * pitch
}

impl Carousel {
    /// The pointer scrolled the row: let it move freely and settle once it stops.
    pub fn wheel(&mut self) {
        self.wheeled = Some(Instant::now());
        self.glide = None;
    }

    /// Glide so item `index` sits on the column's left edge, or as near as the row allows.
    pub fn show(&mut self, index: usize, pitch: f32) {
        self.glide_to(index as f32 * pitch, Instant::now());
        self.wheeled = None;
    }

    /// Keyboard focus is on item `index`: bring it onto the column once, when it arrives.
    pub fn follow(&mut self, index: usize, pitch: f32) {
        if self.focused != Some(index) {
            self.focused = Some(index);
            self.show(index, pitch);
        }
    }

    /// Frames are needed while the row waits to settle or glides.
    pub fn moving(&self, reduced: bool) -> bool {
        self.wheeled.is_some()
            || self
                .glide
                .is_some_and(|glide| glide.moving(Instant::now(), reduced))
    }

    /// How far the row has scrolled.
    pub fn position(&self) -> f32 {
        -f32::from(self.scroll.offset().x)
    }

    fn glide_to(&mut self, target: f32, now: Instant) {
        let target = target.clamp(0.0, f32::from(self.scroll.max_offset().x).max(0.0));
        let mut glide = Tween::fixed(self.position());
        glide.retarget(target, DURATION_DELIBERATE, Some(EASE_EDITORIAL), now);
        self.glide = Some(glide);
    }

    /// Advance the settle and the glide; call once per frame before layout.
    pub fn settle(&mut self, pitch: f32, reduced: bool) {
        let now = Instant::now();
        if self
            .wheeled
            .is_some_and(|at| now.duration_since(at) >= SETTLE_AFTER)
        {
            self.wheeled = None;
            self.glide_to(rest(self.position(), pitch), now);
        }
        if let Some(glide) = self.glide {
            self.scroll
                .set_offset(point(px(-glide.value(now, reduced)), px(0.0)));
            if !glide.moving(now, reduced) {
                self.glide = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rest;

    #[test]
    fn the_row_rests_with_the_nearest_item_on_the_column() {
        let pitch = 372.0;
        assert_eq!(rest(0.0, pitch), 0.0);
        assert_eq!(rest(pitch * 0.4, pitch), 0.0);
        assert_eq!(rest(pitch * 0.6, pitch), pitch);
        assert_eq!(rest(pitch * 2.49, pitch), pitch * 2.0);
    }
}
