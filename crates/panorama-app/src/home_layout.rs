/// Height of a tile as a share of its width: 16:9.
pub const TILE_RATIO: f32 = 9.0 / 16.0;

/// Columns on the widest screens, so tiles never shrink to thumbnails.
const MAX_COLUMNS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct Grid {
    /// Columns in each virtualized row.
    pub columns: usize,
    /// Fixed tile width in logical pixels.
    pub width: f32,
    /// Space between tiles, the column's.
    pub gap: f32,
}
impl Grid {
    /// The most columns whose tiles stay wide enough: 280 up to three, 300 for four, and 400
    /// from five on, up to eight. Tiles stretch so the grid's right edge meets the column's.
    pub fn fit(col: crate::theme::Column) -> Self {
        let (column_width, gap) = (col.width.max(0.0), col.gap);
        let min_tile = |columns: usize| match columns {
            0..=3 => 280.0,
            4 => 300.0,
            _ => 400.0,
        };
        let columns = (2..=MAX_COLUMNS)
            .take_while(|&n| column_width >= n as f32 * min_tile(n) + (n - 1) as f32 * gap)
            .last()
            .unwrap_or(1);
        let gaps = gap * (columns - 1) as f32;
        Self {
            columns,
            width: ((column_width - gaps) / columns as f32).floor(),
            gap,
        }
    }
    pub fn height(self) -> f32 {
        (self.width * TILE_RATIO).round()
    }
    pub fn total_width(self) -> f32 {
        self.width * self.columns as f32 + self.gap * self.columns.saturating_sub(1) as f32
    }
    /// Space between rows, about a tenth of a tile so larger cards breathe more.
    pub fn row_gap(self) -> f32 {
        (self.width * 0.1).round()
    }
    /// Fixed virtualization stride including the row gap.
    pub fn stride(self) -> f32 {
        self.height() + self.row_gap()
    }
    /// Distance from one tile's left edge to the next.
    pub fn pitch(self) -> f32 {
        self.width + self.gap
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
            (1217.0, 3, 401.0),
            (1218.0, 4, 300.0),
            (1270.0, 4, 313.0),
            (1457.0, 4, 359.0),
            (1458.0, 4, 360.0),
            (1500.0, 4, 370.0),
            (1720.0, 4, 425.0),
            (2023.0, 4, 501.0),
            (2024.0, 5, 400.0),
            (2200.0, 5, 435.0),
            (3640.0, 8, 449.0),
            (6000.0, 8, 744.0),
        ] {
            let grid = Grid::fit(crate::theme::Column {
                left: 0.0,
                width: column,
                gap: 6.0,
            });
            assert_eq!((grid.columns, grid.width), (columns, width), "{column}");
        }
    }
    #[test]
    fn tiles_are_sixteen_by_nine() {
        assert_eq!(
            Grid {
                columns: 3,
                width: 480.0,
                gap: 24.0
            }
            .height(),
            270.0
        );
    }
}
