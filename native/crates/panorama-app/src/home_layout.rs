/// Hairline between tiles in a row.
pub const GAP: f32 = 4.0;
/// Space between rows, so each row reads as its own line of films.
pub const ROW_GAP: f32 = 32.0;

#[derive(Clone, Copy, Debug)]
pub struct Grid {
    /// Columns in each virtualized row.
    pub columns: usize,
    /// Fixed tile width in logical pixels.
    pub width: f32,
}
impl Grid {
    /// As many columns of at least `MIN_TILE` as fit (1..=4), stretched to fill the column so the
    /// grid's right edge meets the column's.
    pub fn fit(column_width: f32) -> Self {
        const MIN_TILE: f32 = 280.0;
        let column_width = column_width.max(0.0);
        let columns = (((column_width + GAP) / (MIN_TILE + GAP)).floor() as usize).clamp(1, 4);
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
    /// Fixed virtualization stride including the row gap.
    pub fn stride(self) -> f32 {
        self.height() + ROW_GAP
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
            (920.0, 3, 304.0),
            (1131.0, 3, 374.0),
            (1132.0, 4, 280.0),
            (1500.0, 4, 372.0),
        ] {
            let grid = Grid::fit(column);
            assert_eq!((grid.columns, grid.width), (columns, width), "{column}");
        }
    }
}
