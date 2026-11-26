/// Width of the navigation sidebar every route except the player reserves.
pub const SIDEBAR_WIDTH: f32 = 80.0;
/// Each margin's share of the window, as on MUBI: the page is one centred column.
const MARGIN_SHARE: f32 = 0.115;
/// The narrowest margin: the sidebar, which sits inside the left one, and room beside it.
const MARGIN_MIN: f32 = SIDEBAR_WIDTH + 24.0;
/// The hairline between cards side by side.
const GAP: f32 = 6.0;

/// The content column every route aligns to: centred in the window between equal margins, the
/// sidebar sitting inside the left one, so the page is balanced against the window's edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub left: f32,
    /// Usable width.
    pub width: f32,
    /// Space between cards side by side.
    pub gap: f32,
}

impl Column {
    /// Place the column for a window width.
    pub fn at(window_width: f32) -> Self {
        let margin = (window_width * MARGIN_SHARE).round().max(MARGIN_MIN);
        Self {
            left: margin,
            width: (window_width - margin * 2.0).max(0.0),
            gap: GAP,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_column_is_centred_with_margins_that_hold_the_sidebar() {
        for (window, left, width) in [
            (100.0, 104.0, 0.0),
            (800.0, 104.0, 592.0),
            (1280.0, 147.0, 986.0),
            (1440.0, 166.0, 1108.0),
            (1920.0, 221.0, 1478.0),
        ] {
            let col = Column::at(window);
            assert_eq!(
                (col.left, col.width, col.gap),
                (left, width, 6.0),
                "{window}"
            );
        }
    }
}
