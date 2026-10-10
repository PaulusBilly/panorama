//! CSS Color 4 conversion, clipped to the display's sRGB gamut.
use gpui::Rgba;

/// Convert original OKLCH coordinates to encoded sRGB, preserving alpha.
pub fn oklch(lightness: f32, chroma: f32, hue: f32, alpha: f32) -> Rgba {
    let angle = f64::from(hue).to_radians();
    let a = f64::from(chroma) * angle.cos();
    let b = f64::from(chroma) * angle.sin();
    let l = f64::from(lightness);
    let ll = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let mm = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let ss = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    let encode = |linear: f64| {
        let linear = linear.clamp(0.0, 1.0);
        (if linear <= 0.0031308 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        }) as f32
    };
    Rgba {
        r: encode(4.0767416621 * ll - 3.3077115913 * mm + 0.2309699292 * ss),
        g: encode(-1.2684380046 * ll + 2.6097574011 * mm - 0.3413193965 * ss),
        b: encode(-0.0041960863 * ll - 0.7034186147 * mm + 1.7076147010 * ss),
        a: alpha.clamp(0.0, 1.0),
    }
}

/// Convert a six-digit hexadecimal CSS color.
pub const fn hex(value: u32) -> Rgba {
    Rgba {
        r: ((value >> 16) & 255) as f32 / 255.0,
        g: ((value >> 8) & 255) as f32 / 255.0,
        b: (value & 255) as f32 / 255.0,
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assert_rgb(color: Rgba, expected: [u8; 3]) {
        for (channel, expected) in [color.r, color.g, color.b].into_iter().zip(expected) {
            assert!(((channel * 255.0).round() - f32::from(expected)).abs() <= 1.0);
        }
    }
    #[test]
    fn reference_colors() {
        assert_rgb(oklch(1.0, 0.0, 0.0, 1.0), [255, 255, 255]);
        assert_rgb(oklch(0.2, 0.0, 0.0, 1.0), [22, 22, 22]);
        // Independently evaluated in double precision using CSS Color 4's XYZ-D65 matrices: #9a2e24.
        assert_rgb(oklch(0.464, 0.145, 29.093, 1.0), [154, 46, 36]);
    }
    #[test]
    fn clipping_and_alpha() {
        let color = oklch(0.7, 1.0, 30.0, 0.18);
        assert_eq!(color.a, 0.18);
        assert!(
            [color.r, color.g, color.b]
                .iter()
                .all(|x| (0.0..=1.0).contains(x))
        );
        assert_eq!(hex(0xebebeb).r, 235.0 / 255.0);
    }
}
