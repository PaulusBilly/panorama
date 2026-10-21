/// Space between tiles, the same across columns and rows.
pub const GAP: f32 = 4.0;

/// Fixed-size tiles packed into the content column.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    /// Columns in each virtualized row.
    pub columns: usize,
    /// Fixed tile width in logical pixels.
    pub width: f32,
}
impl Grid {
    /// Columns that fit: tile 372 (335 when the column is under 760), 1..=4 columns.
    pub fn fit(column_width: f32) -> Self {
        let tile: f32 = if column_width < 760.0 { 335.0 } else { 372.0 };
        let columns = (((column_width + GAP) / (tile + GAP)).floor() as usize).clamp(1, 4);
        Self {
            columns,
            // A single column narrower than a tile shrinks the tile rather than overflowing.
            width: tile.min(column_width.max(0.0)),
        }
    }
    /// FilmCard's 404:245 aspect ratio.
    pub fn height(self) -> f32 {
        self.width * 245.0 / 404.0
    }
    /// Total row width, including column gaps.
    pub fn total_width(self) -> f32 {
        self.width * self.columns as f32 + GAP * self.columns.saturating_sub(1) as f32
    }
    /// Fixed virtualization stride including the row gap.
    pub fn stride(self) -> f32 {
        self.height() + GAP
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
            (673.0, 1, 335.0),
            (674.0, 2, 335.0),
            (759.0, 2, 335.0),
            (760.0, 2, 372.0),
            (1123.0, 2, 372.0),
            (1124.0, 3, 372.0),
            (1500.0, 4, 372.0),
            (3000.0, 4, 372.0),
        ] {
            let grid = Grid::fit(column);
            assert_eq!((grid.columns, grid.width), (columns, width), "{column}");
        }
    }
}
