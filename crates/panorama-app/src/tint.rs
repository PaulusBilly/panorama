//! The color a still reads as, for placeholders shown before larger artwork arrives.
use gpui::{Hsla, Rgba};

/// Pixels sampled from one image; a still's color survives far coarser sampling than this.
const SAMPLES: usize = 4096;
/// Placeholders sit under white type and a dark canvas: a bright still must not flash, and a
/// dark one must still read as a color after the scrims.
const LIGHTNESS: std::ops::RangeInclusive<f32> = 0.16..=0.3;

/// The heaviest 12-bit color bin, weighted toward vivid pixels so a small sunset beats a large
/// grey sky. Letterbox black and blown-out white don't count; `None` when nothing else remains.
pub fn dominant(rgba: &[u8]) -> Option<Hsla> {
    let pixels = rgba.as_chunks::<4>().0;
    let mut bins = vec![[0.0_f32; 4]; 4096];
    for &[r, g, b, a] in pixels.iter().step_by((pixels.len() / SAMPLES).max(1)) {
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        if a < 128 || max < 48 || min > 232 {
            continue;
        }
        let bin =
            &mut bins[usize::from(r >> 4) << 8 | usize::from(g >> 4) << 4 | usize::from(b >> 4)];
        let weight = 0.05 + f32::from(max - min) / 255.0;
        bin[0] += f32::from(r) * weight;
        bin[1] += f32::from(g) * weight;
        bin[2] += f32::from(b) * weight;
        bin[3] += weight;
    }
    let [r, g, b, weight] = bins.into_iter().max_by(|a, b| a[3].total_cmp(&b[3]))?;
    if weight == 0.0 {
        return None;
    }
    let scale = weight * 255.0;
    let color = Hsla::from(Rgba {
        r: r / scale,
        g: g / scale,
        b: b / scale,
        a: 1.0,
    });
    Some(Hsla {
        l: color.l.clamp(*LIGHTNESS.start(), *LIGHTNESS.end()),
        ..color
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(colors: &[([u8; 3], usize)]) -> Vec<u8> {
        colors
            .iter()
            .flat_map(|&([r, g, b], count)| [r, g, b, 255].repeat(count))
            .collect()
    }

    #[test]
    fn a_vivid_minority_beats_a_grey_majority() {
        let color = dominant(&image(&[([128, 128, 128], 80), ([200, 40, 30], 20)])).unwrap();
        assert!(color.h < 0.02 && color.s > 0.5, "{color:?}");
    }
    #[test]
    fn lightness_stays_in_the_placeholder_range() {
        assert_eq!(dominant(&image(&[([250, 200, 40], 9)])).unwrap().l, 0.3);
        assert_eq!(dominant(&image(&[([10, 60, 40], 9)])).unwrap().l, 0.16);
    }
    #[test]
    fn near_black_shadows_lose_to_the_color_that_shows() {
        let color = dominant(&image(&[([8, 18, 29], 90), ([90, 60, 40], 10)])).unwrap();
        assert!(color.h < 0.1, "{color:?}");
    }
    #[test]
    fn letterbox_bars_do_not_count() {
        let color = dominant(&image(&[([0, 0, 0], 90), ([30, 60, 200], 10)])).unwrap();
        assert!((color.h - 0.634).abs() < 0.01, "{color:?}");
    }
    #[test]
    fn empty_black_white_and_transparent_images_have_no_color() {
        assert_eq!(dominant(&[]), None);
        assert_eq!(
            dominant(&image(&[([0, 0, 0], 4), ([255, 255, 255], 4)])),
            None
        );
        assert_eq!(dominant(&[200, 40, 30, 0]), None);
    }
}
