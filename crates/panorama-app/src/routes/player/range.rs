//! Geometry and scrub state of the seek and volume ranges (`.player-range` in globals.css).

/// The two ranges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeId {
    /// Volume, in the top bar.
    Volume,
    /// Seek, in the bottom bar.
    Seek,
}

impl RangeId {
    /// The range's fixed dimensions.
    pub fn look(self) -> Look {
        match self {
            Self::Volume => VOLUME,
            Self::Seek => SEEK,
        }
    }
}

/// Pointer hit height of every range.
pub const HIT_HEIGHT: f32 = 20.0;
/// Keyboard step for both ranges (seconds for seek, percent for volume).
pub const KEY_STEP: f64 = 5.0;

/// The two ranges' fixed dimensions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Track height at rest.
    pub track: f32,
    /// Track height while hovered, focused or dragged.
    pub track_active: f32,
    /// Thumb box width and height.
    pub thumb: f32,
    /// Thumb dot scale at rest.
    pub dot_scale: f32,
    /// Thumb dot opacity at rest.
    pub dot_opacity: f32,
}

/// The volume range.
pub const VOLUME: Look = Look {
    track: 3.0,
    track_active: 5.0,
    thumb: 12.0,
    dot_scale: 0.25,
    dot_opacity: 0.0,
};
/// The seek range: thicker, with an always visible thumb dot.
pub const SEEK: Look = Look {
    track: 4.0,
    track_active: 6.0,
    thumb: 14.0,
    dot_scale: 0.86,
    dot_opacity: 1.0,
};

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount.clamp(0.0, 1.0)
}

impl Look {
    /// Track height for an emphasis of 0 (rest) to 1 (hover, focus or drag).
    pub fn track_height(self, emphasis: f32) -> f32 {
        lerp(self.track, self.track_active, emphasis)
    }
    /// Thumb dot scale for an emphasis.
    pub fn dot_scale(self, emphasis: f32) -> f32 {
        lerp(self.dot_scale, 1.0, emphasis)
    }
    /// Thumb dot opacity for an emphasis.
    pub fn dot_opacity(self, emphasis: f32) -> f32 {
        lerp(self.dot_opacity, 1.0, emphasis)
    }
    /// The fraction a pointer at `x` (relative to the control's left edge) selects.
    pub fn fraction_at(self, x: f32, width: f32) -> f32 {
        let travel = width - self.thumb;
        if travel <= 0.0 || !x.is_finite() {
            return 0.0;
        }
        ((x - self.thumb / 2.0) / travel).clamp(0.0, 1.0)
    }
}

/// Played and buffered fractions of the seek range.
pub fn fractions(duration: Option<f64>, shown: f64, buffered: Option<f64>) -> (f32, f32) {
    let Some(duration) = duration.filter(|duration| *duration > 0.0) else {
        return (0.0, 0.0);
    };
    let fraction = |value: f64| (value / duration).clamp(0.0, 1.0) as f32;
    let played = fraction(shown);
    (
        played,
        buffered.map_or(played, |end| played.max(fraction(end))),
    )
}

/// Seek drag state: previews time without seeking, commits on release or blur.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scrub {
    preview: Option<f64>,
    dragging: bool,
}

impl Scrub {
    /// A pointer press starts a drag at `value`.
    pub fn begin(&mut self, value: f64, duration: Option<f64>) {
        self.dragging = true;
        self.preview(value, duration);
    }
    /// Preview a time; ignored while the duration is unknown.
    pub fn preview(&mut self, value: f64, duration: Option<f64>) {
        if let Some(duration) = duration.filter(|duration| *duration > 0.0)
            && value.is_finite()
        {
            self.preview = Some(value.clamp(0.0, duration));
        }
    }
    /// Take the previewed time to seek to, ending the drag.
    pub fn commit(&mut self) -> Option<f64> {
        self.dragging = false;
        self.preview.take()
    }
    /// Drop the preview without seeking (pointer cancel or Escape).
    pub fn cancel(&mut self) {
        self.dragging = false;
        self.preview = None;
    }
    /// Whether the pointer is down.
    pub fn dragging(&self) -> bool {
        self.dragging
    }
    /// Whether a preview is waiting to commit.
    pub fn pending(&self) -> bool {
        self.preview.is_some()
    }
    /// The time the labels and fill show.
    pub fn shown(&self, time: f64) -> f64 {
        self.preview.unwrap_or(time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbs_align_to_the_edges_of_the_control() {
        for (look, width) in [(SEEK, 400.0), (VOLUME, 96.0)] {
            assert_eq!(look.fraction_at(look.thumb / 2.0, width), 0.0);
            assert_eq!(look.fraction_at(width - look.thumb / 2.0, width), 1.0);
            let middle = look.fraction_at(width / 2.0, width);
            assert!((middle - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn pointer_positions_clamp() {
        assert_eq!(SEEK.fraction_at(-30.0, 300.0), 0.0);
        assert_eq!(SEEK.fraction_at(900.0, 300.0), 1.0);
        assert_eq!(SEEK.fraction_at(5.0, 10.0), 0.0);
        assert_eq!(SEEK.fraction_at(f32::NAN, 300.0), 0.0);
    }

    #[test]
    fn emphasis_blends_the_rest_and_active_looks() {
        assert_eq!(SEEK.track_height(0.0), 4.0);
        assert_eq!(SEEK.track_height(1.0), 6.0);
        assert_eq!(VOLUME.track_height(1.0), 5.0);
        assert_eq!(
            (VOLUME.dot_scale(0.0), VOLUME.dot_opacity(0.0)),
            (0.25, 0.0)
        );
        assert_eq!((VOLUME.dot_scale(1.0), VOLUME.dot_opacity(1.0)), (1.0, 1.0));
        assert_eq!((SEEK.dot_scale(0.0), SEEK.dot_opacity(0.0)), (0.86, 1.0));
        assert_eq!(SEEK.dot_scale(1.0), 1.0);
    }

    #[test]
    fn fractions_clamp_and_buffered_never_trails_played() {
        assert_eq!(fractions(None, 10.0, Some(20.0)), (0.0, 0.0));
        assert_eq!(fractions(Some(0.0), 10.0, None), (0.0, 0.0));
        assert_eq!(fractions(Some(100.0), 25.0, None), (0.25, 0.25));
        assert_eq!(fractions(Some(100.0), 25.0, Some(10.0)), (0.25, 0.25));
        assert_eq!(fractions(Some(100.0), 25.0, Some(60.0)), (0.25, 0.6));
        assert_eq!(fractions(Some(100.0), 250.0, Some(900.0)), (1.0, 1.0));
    }

    #[test]
    fn scrub_previews_then_commits_once() {
        let mut scrub = Scrub::default();
        assert_eq!(scrub.shown(12.0), 12.0);
        scrub.begin(40.0, Some(100.0));
        assert!(scrub.dragging() && scrub.pending());
        assert_eq!(scrub.shown(12.0), 40.0);
        scrub.preview(250.0, Some(100.0));
        assert_eq!(scrub.shown(12.0), 100.0);
        assert_eq!(scrub.commit(), Some(100.0));
        assert!(!scrub.dragging() && !scrub.pending());
        assert_eq!(scrub.commit(), None);
        assert_eq!(scrub.shown(12.0), 12.0);
    }

    #[test]
    fn scrub_cancel_drops_the_preview_and_unknown_durations_ignore_input() {
        let mut scrub = Scrub::default();
        scrub.begin(40.0, Some(100.0));
        scrub.cancel();
        assert_eq!(scrub.commit(), None);
        scrub.begin(40.0, None);
        scrub.preview(10.0, Some(0.0));
        scrub.preview(f64::NAN, Some(100.0));
        assert_eq!(scrub.commit(), None);
    }
}
