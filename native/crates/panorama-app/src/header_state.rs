use crate::theme::Theme;
use gpui::Rgba;

/// Home header color state, matching the nested TSX conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderColor {
    /// Artwork and featured copy are ready over the hero.
    HeroReady,
    /// Hero surface overlay is still visible.
    HeroLoading,
    /// The scroll position is below the hero.
    Below,
}
impl HeaderColor {
    /// Resolve Home's nested header-color condition.
    pub fn at(over_hero: bool, ready: bool) -> Self {
        if !over_hero {
            Self::Below
        } else if ready {
            Self::HeroReady
        } else {
            Self::HeroLoading
        }
    }
    /// Foreground color and logo tint.
    pub fn foreground(self, theme: Theme) -> Rgba {
        if self == Self::HeroReady {
            theme.inverse
        } else {
            theme.ink
        }
    }
    /// Solid background appears only below the hero.
    pub fn background(self, theme: Theme) -> Rgba {
        if self == Self::Below {
            theme.canvas
        } else {
            theme.canvas.opacity(0.0)
        }
    }
}

/// Sticky state retained independently of scroll event frequency.
#[derive(Default)]
pub struct Sticky {
    /// Header is hidden while scrolling down past its own height.
    pub hidden: bool,
    /// Hero's bottom remains below the header's bottom.
    pub over_hero: bool,
    previous: f32,
}
impl Sticky {
    /// Initialize the fixed header over Home before the first scroll event.
    pub fn home() -> Self {
        Self {
            over_hero: true,
            ..Self::default()
        }
    }
    /// Apply an actual scroll position after GPUI has updated its scroll handle.
    pub fn scroll(&mut self, top: f32, header_height: f32, hero_height: f32) {
        self.over_hero = hero_height - top > header_height;
        if top <= 0.0 || top < self.previous {
            self.hidden = false;
        } else if top > self.previous && top > header_height {
            self.hidden = true;
        }
        self.previous = top;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_color_conditions() {
        assert_eq!(HeaderColor::at(true, true), HeaderColor::HeroReady);
        assert_eq!(HeaderColor::at(true, false), HeaderColor::HeroLoading);
        for ready in [false, true] {
            assert_eq!(HeaderColor::at(false, ready), HeaderColor::Below);
        }
    }
    #[test]
    fn sticky_scroll_sequence() {
        let mut state = Sticky::default();
        for (top, hidden, over) in [
            (0.0, false, true),
            (30.0, false, true),
            (60.0, false, true),
            (61.0, true, true),
            (768.0, true, false),
            (568.0, false, true),
            (568.0, false, true),
            (700.0, true, true),
            (0.0, false, true),
        ] {
            state.scroll(top, 60.0, 768.0);
            assert_eq!((state.hidden, state.over_hero), (hidden, over), "{top}");
        }
    }
}
