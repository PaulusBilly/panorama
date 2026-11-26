/// Width of the navigation sidebar every route except the player reserves.
pub const SIDEBAR_WIDTH: f32 = 80.0;

/// The content column every route aligns to: the window after the sidebar, less an equal gutter
/// on both sides, so wide screens get more columns instead of an empty right edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub left: f32,
    /// Usable width.
    pub width: f32,
    /// Space between cards side by side. The gutter is twice it, so a row's next card shows one
    /// gap's worth past the column, and a carousel arrow centres in the gutter over that sliver.
    pub gap: f32,
}

impl Column {
    /// Place the column for a window width: cards 24, 20 or 12 px apart, and a gutter of twice
    /// that on both sides.
    pub fn at(window_width: f32) -> Self {
        let gap = if window_width > 1100.0 {
            24.0
        } else if window_width > 700.0 {
            20.0
        } else {
            12.0
        };
        let gutter = gap * 2.0;
        let left = SIDEBAR_WIDTH + gutter;
        Self {
            left,
            width: (window_width - left - gutter).max(0.0),
            gap,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_gutter_is_twice_the_gap_on_both_sides() {
        for window in [480.0, 800.0, 1440.0, 1920.0] {
            let col = Column::at(window);
            assert_eq!(col.left - SIDEBAR_WIDTH, col.gap * 2.0, "{window}");
            assert_eq!(window - col.left - col.width, col.gap * 2.0, "{window}");
        }
    }

    #[test]
    fn column_breakpoints() {
        for (width, left, column, gap) in [
            (480.0, 104.0, 352.0, 12.0),
            (700.0, 104.0, 572.0, 12.0),
            (701.0, 120.0, 541.0, 20.0),
            (1100.0, 120.0, 940.0, 20.0),
            (1101.0, 128.0, 925.0, 24.0),
            (1440.0, 128.0, 1264.0, 24.0),
            (1920.0, 128.0, 1744.0, 24.0),
            (100.0, 104.0, 0.0, 12.0),
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
