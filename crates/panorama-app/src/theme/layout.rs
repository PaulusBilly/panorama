/// Width of the navigation sidebar every route except the player reserves.
pub const SIDEBAR_WIDTH: f32 = 80.0;

/// The content column every route aligns to: from the sidebar's edge to a right margin of two
/// card gaps, which the next card in a scrolling row peeks into by one gap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub left: f32,
    /// Usable width.
    pub width: f32,
    /// Space between cards side by side.
    pub gap: f32,
}

impl Column {
    /// Place the column for a window width, with cards 24, 20 or 12 px apart and a right margin
    /// twice that.
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
            width: (window_width - SIDEBAR_WIDTH - gap * 2.0).max(0.0),
            gap,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_page_starts_at_the_sidebar_and_ends_two_gaps_from_the_edge() {
        for window in [480.0, 800.0, 1440.0, 1920.0] {
            let col = Column::at(window);
            assert_eq!(col.left, SIDEBAR_WIDTH, "{window}");
            assert_eq!(window - col.left - col.width, col.gap * 2.0, "{window}");
        }
    }

    #[test]
    fn column_breakpoints() {
        for (width, left, column, gap) in [
            (480.0, 80.0, 376.0, 12.0),
            (700.0, 80.0, 596.0, 12.0),
            (701.0, 80.0, 581.0, 20.0),
            (1101.0, 80.0, 973.0, 24.0),
            (1440.0, 80.0, 1312.0, 24.0),
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
