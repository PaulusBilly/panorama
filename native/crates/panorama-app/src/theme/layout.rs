//! Responsive CSS layout rules.

/// Page gutter in logical pixels, including CSS's inclusive max-width queries.
pub fn page_gutter(width: f32) -> f32 {
    if width <= 700.0 {
        20.0
    } else if width <= 1100.0 {
        48.0
    } else {
        82.0
    }
}

/// Dialog inset in logical pixels.
pub fn dialog_inset(width: f32) -> f32 {
    if width <= 700.0 { 16.0 } else { 24.0 }
}

/// Width of the Electron content-container at the given viewport width.
pub fn content_width(width: f32) -> f32 {
    let max = if width < 720.0 {
        335.0
    } else if width < 850.0 {
        674.0
    } else if width < 1220.0 {
        748.0
    } else if width < 1640.0 {
        1124.0
    } else {
        1500.0
    };
    (width - 2.0 * (width * 0.0427).clamp(20.0, 70.0))
        .max(0.0)
        .min(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_breakpoints() {
        for (width, expected) in [
            (719.0, 335.0),
            (720.0, 658.512),
            (849.0, 674.0),
            (850.0, 748.0),
            (1219.0, 748.0),
            (1220.0, 1115.812),
            (1639.0, 1124.0),
            (1640.0, 1500.0),
            (1800.0, 1500.0),
            (320.0, 280.0),
            (0.0, 0.0),
        ] {
            assert!((content_width(width) - expected).abs() < 0.01, "{width}");
        }
    }
    #[test]
    fn gutters_and_insets() {
        for (width, gutter, inset) in [
            (699.0, 20.0, 16.0),
            (700.0, 20.0, 16.0),
            (701.0, 48.0, 24.0),
            (1100.0, 48.0, 24.0),
            (1101.0, 82.0, 24.0),
        ] {
            assert_eq!(page_gutter(width), gutter);
            assert_eq!(dialog_inset(width), inset);
        }
    }
}
