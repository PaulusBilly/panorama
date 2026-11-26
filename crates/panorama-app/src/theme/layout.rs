/// Width of the navigation sidebar every route except the player reserves.
pub const SIDEBAR_WIDTH: f32 = 80.0;
/// The sidebar's icons, centred in it.
pub const SIDEBAR_GLYPH: f32 = 24.0;
/// The empty space between the sidebar's icons and the page. The sidebar shares the page's
/// background, so the eye measures from its icons, not its edge: the right margin is as wide.
pub const MARGIN: f32 = (SIDEBAR_WIDTH - SIDEBAR_GLYPH) / 2.0;

/// The content column every route aligns to: from the sidebar's edge to [`MARGIN`] from the
/// window's, so the page sits evenly between the sidebar's icons and the window edge.
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
            width: (window_width - SIDEBAR_WIDTH - MARGIN).max(0.0),
            gap,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_page_sits_as_far_from_the_window_edge_as_from_the_sidebar_icons() {
        assert_eq!(MARGIN, 28.0);
        for window in [480.0, 800.0, 1440.0, 1920.0] {
            let col = Column::at(window);
            let icons_end = (SIDEBAR_WIDTH + SIDEBAR_GLYPH) / 2.0;
            assert_eq!(col.left - icons_end, MARGIN, "{window}");
            assert_eq!(window - col.left - col.width, MARGIN, "{window}");
        }
    }

    #[test]
    fn column_breakpoints() {
        for (width, left, column, gap) in [
            (480.0, 80.0, 372.0, 12.0),
            (700.0, 80.0, 592.0, 12.0),
            (701.0, 80.0, 593.0, 20.0),
            (1101.0, 80.0, 993.0, 24.0),
            (1440.0, 80.0, 1332.0, 24.0),
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
