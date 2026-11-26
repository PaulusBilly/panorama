/// Width of the navigation sidebar every route except the player reserves.
pub const SIDEBAR_WIDTH: f32 = 80.0;

/// The content column every route aligns to: from the sidebar's edge to a right margin as wide as
/// the sidebar, so the page sits framed between two equal bands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub left: f32,
    /// Usable width.
    pub width: f32,
    /// Space between cards side by side.
    pub gap: f32,
}

impl Column {
    /// Place the column for a window width, with cards 24, 20 or 12 px apart.
    pub fn at(window_width: f32) -> Self {
        let gap = if window_width > 1100.0 {
            24.0
        } else if window_width > 700.0 {
            20.0
        } else {
            12.0
        };
        Self {
            left: SIDEBAR_WIDTH,
            width: (window_width - SIDEBAR_WIDTH * 2.0).max(0.0),
            gap,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_right_margin_matches_the_sidebar() {
        for window in [480.0, 800.0, 1440.0, 1920.0] {
            let col = Column::at(window);
            assert_eq!(col.left, SIDEBAR_WIDTH, "{window}");
            assert_eq!(window - col.left - col.width, SIDEBAR_WIDTH, "{window}");
        }
    }

    #[test]
    fn column_breakpoints() {
        for (width, left, column, gap) in [
            (480.0, 80.0, 320.0, 12.0),
            (700.0, 80.0, 540.0, 12.0),
            (701.0, 80.0, 541.0, 20.0),
            (1101.0, 80.0, 941.0, 24.0),
            (1440.0, 80.0, 1280.0, 24.0),
            (100.0, 80.0, 0.0, 12.0),
        ] {
            assert_eq!(
                Column::at(width),
                Column {
                    left,
                    width: column,
                    gap
                },
                "{width}"
            );
        }
    }
}
