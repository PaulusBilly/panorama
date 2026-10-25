//! The custom colour picker's model: hue, saturation and brightness, each 0..=1.
use crate::playback::subtitle_prefs::Rgb;

/// The part of the picker a drag moves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Part {
    /// The saturation and brightness square.
    Field,
    /// The hue strip.
    Hue,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    pub fn from_rgb(rgb: Rgb) -> Self {
        let [r, g, b] = rgb.map(|c| f32::from(c) / 255.0);
        let max = r.max(g).max(b);
        let delta = max - r.min(g).min(b);
        let h = if delta == 0.0 {
            0.0
        } else if max == r {
            ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        } / 6.0;
        Self {
            h,
            s: if max == 0.0 { 0.0 } else { delta / max },
            v: max,
        }
    }

    pub fn to_rgb(self) -> Rgb {
        let h = self.h.clamp(0.0, 1.0) * 6.0;
        let (s, v) = (self.s.clamp(0.0, 1.0), self.v.clamp(0.0, 1.0));
        let channel = |n: f32| {
            let k = (n + h).rem_euclid(6.0);
            let c = v - v * s * k.min(4.0 - k).clamp(0.0, 1.0);
            (c * 255.0).round() as u8
        };
        [channel(5.0), channel(3.0), channel(1.0)]
    }

    /// The fully saturated, full brightness colour of this hue.
    pub fn hue_rgb(h: f32) -> Rgb {
        Self { h, s: 1.0, v: 1.0 }.to_rgb()
    }

    /// Move to a typed colour, keeping the hue (and saturation) a grey or black does not carry,
    /// so the thumbs do not jump while the colour stays the same.
    pub fn follow(self, rgb: Rgb) -> Self {
        if self.to_rgb() == rgb {
            return self;
        }
        let mut next = Self::from_rgb(rgb);
        if next.s == 0.0 || next.v == 0.0 {
            next.h = self.h;
        }
        if next.v == 0.0 {
            next.s = self.s;
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_round_trip_through_hsv() {
        for rgb in [
            [0xff, 0xff, 0xff],
            [0xff, 0xe0, 0x66],
            [0xa3, 0xe6, 0xa1],
            [0x8d, 0xe4, 0xff],
            [0xff, 0xb5, 0xd5],
            [0x12, 0x34, 0x56],
            [0, 0, 0],
        ] {
            assert_eq!(Hsv::from_rgb(rgb).to_rgb(), rgb);
        }
        assert_eq!(Hsv::hue_rgb(0.0), [255, 0, 0]);
        assert_eq!(Hsv::hue_rgb(1.0 / 3.0), [0, 255, 0]);
        assert_eq!(Hsv::hue_rgb(1.0), [255, 0, 0]);
    }

    #[test]
    fn greys_and_black_keep_the_picked_hue() {
        let blue = Hsv::from_rgb([0, 0, 255]);
        let black = blue.follow([0, 0, 0]);
        assert_eq!((black.h, black.s, black.v), (blue.h, 1.0, 0.0));
        let grey = blue.follow([0x80, 0x80, 0x80]);
        assert_eq!((grey.h, grey.s), (blue.h, 0.0));
        let held = Hsv {
            h: 0.3,
            s: 0.0,
            v: 1.0,
        };
        assert_eq!(held.follow([255, 255, 255]), held);
    }
}
