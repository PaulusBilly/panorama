//! Responsive layout rules.

/// Width of the navigation sidebar every route except the player reserves.
pub const SIDEBAR_WIDTH: f32 = 80.0;

/// Dialog inset in logical pixels.
pub fn dialog_inset(width: f32) -> f32 {
    if width <= 700.0 { 16.0 } else { 24.0 }
}

/// The content column every route aligns to. Left-aligned after the sidebar, not centred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    /// Distance from the window's left edge.
    pub left: f32,
    /// Usable width, at most 1500.
    pub width: f32,
}

impl Column {
    /// Place the column for a window width, with a 60, 40 or 24 px gutter on both sides.
    pub fn at(window_width: f32) -> Self {
        let gutter = if window_width > 1100.0 {
            60.0
        } else if window_width > 700.0 {
            40.0
        } else {
            24.0
        };
        let left = SIDEBAR_WIDTH + gutter;
        Self {
            left,
            width: (window_width - left - gutter).clamp(0.0, 1500.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn column_breakpoints() {
        for (width, left, column) in [
            (480.0, 104.0, 352.0),
            (700.0, 104.0, 572.0),
            (701.0, 120.0, 541.0),
            (1100.0, 120.0, 940.0),
            (1101.0, 140.0, 901.0),
            (1280.0, 140.0, 1080.0),
            (2400.0, 140.0, 1500.0),
            (100.0, 104.0, 0.0),
        ] {
            assert_eq!(
                Column::at(width),
                Column {
                    left,
                    width: column
                },
                "{width}"
            );
        }
    }
    #[test]
    fn dialog_insets() {
        for (width, inset) in [(700.0, 16.0), (701.0, 24.0)] {
            assert_eq!(dialog_inset(width), inset);
        }
    }
}
