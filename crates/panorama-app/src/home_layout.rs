/// Hairline between tiles in a row.
pub const GAP: f32 = 6.0;

/// Height of a tile as a share of its width: 16:9.
pub const TILE_RATIO: f32 = 9.0 / 16.0;

#[derive(Clone, Copy, Debug)]
pub struct Grid {
    /// Columns in each virtualized row.
    pub columns: usize,
    /// Fixed tile width in logical pixels.
    pub width: f32,
}
impl Grid {
    /// Three columns from a 1080-wide window up to wide desktops; four only once each tile can
    /// stay at least 360 wide. Tiles stretch so the grid's right edge meets the column's.
    pub fn fit(column_width: f32) -> Self {
        let column_width = column_width.max(0.0);
        let columns = match column_width {
            w if w >= 4.0 * 360.0 + 3.0 * GAP => 4,
            w if w >= 3.0 * 280.0 + 2.0 * GAP => 3,
            w if w >= 2.0 * 280.0 + GAP => 2,
            _ => 1,
        };
        let gaps = GAP * (columns - 1) as f32;
        Self {
            columns,
            width: ((column_width - gaps) / columns as f32).floor(),
        }
    }
    pub fn height(self) -> f32 {
        (self.width * TILE_RATIO).round()
    }
    pub fn total_width(self) -> f32 {
        self.width * self.columns as f32 + GAP * self.columns.saturating_sub(1) as f32
    }
    /// Space between rows, about a tenth of a tile so larger cards breathe more.
    pub fn row_gap(self) -> f32 {
        (self.width * 0.1).round()
    }
    /// Fixed virtualization stride including the row gap.
    pub fn stride(self) -> f32 {
        self.height() + self.row_gap()
    }
}

/// The page footer, empty for now. Its height is the room a card preview on the last row
/// needs to open fully, since the preview hangs below its tile and the page clips there.
pub fn footer() -> gpui::Div {
    use gpui::{Styled, div, px};
    div().w_full().h(px(240.0)).flex_shrink_0()
}

/// Shared logical viewport and interaction settings for Home elements.
#[derive(Clone, Copy)]
pub struct ViewSettings {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) theme: crate::theme::Theme,
    pub(crate) active: bool,
    pub(crate) keyboard: bool,
    pub(crate) reduced: bool,
}
impl ViewSettings {
    /// The content column for this viewport; derived, never stored.
    pub fn column(&self) -> crate::theme::Column {
        crate::theme::Column::at(self.width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_fits_the_column() {
        for (column, columns, width) in [
            (280.0, 1, 280.0),
            (565.0, 1, 565.0),
            (566.0, 2, 280.0),
            (851.0, 2, 422.0),
            (852.0, 3, 280.0),
            (920.0, 3, 302.0),
            (1270.0, 3, 419.0),
            (1457.0, 3, 481.0),
            (1458.0, 4, 360.0),
            (1500.0, 4, 370.0),
        ] {
            let grid = Grid::fit(column);
            assert_eq!((grid.columns, grid.width), (columns, width), "{column}");
        }
    }
    #[test]
    fn tiles_are_sixteen_by_nine() {
        assert_eq!(
            Grid {
                columns: 3,
                width: 480.0
            }
            .height(),
            270.0
        );
    }
}
