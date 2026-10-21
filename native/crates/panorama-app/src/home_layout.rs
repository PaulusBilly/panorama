/// Hairline between tiles in a row.
pub const GAP: f32 = 4.0;

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
    /// 16:9, the same frame as the Continue row.
    pub fn height(self) -> f32 {
        (self.width * 9.0 / 16.0).round()
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
            (563.0, 1, 563.0),
            (564.0, 2, 280.0),
            (847.0, 2, 421.0),
            (848.0, 3, 280.0),
            (920.0, 3, 304.0),
            (1270.0, 3, 420.0),
            (1451.0, 3, 481.0),
            (1452.0, 4, 360.0),
            (1500.0, 4, 372.0),
        ] {
            let grid = Grid::fit(column);
            assert_eq!((grid.columns, grid.width), (columns, width), "{column}");
        }
    }
}
