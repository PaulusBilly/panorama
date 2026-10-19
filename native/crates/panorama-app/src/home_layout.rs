/// Fixed tile dimensions matching catalogGridClass's strict width queries.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    /// Columns in each virtualized row.
    pub columns: usize,
    /// Fixed tile width in logical pixels.
    pub width: f32,
}
impl Grid {
    /// Compute the exact Electron grid at each breakpoint.
    pub fn at(viewport: f32) -> Self {
        let (columns, width): (usize, f32) = if viewport >= 1640.0 {
            (4, 372.0)
        } else if viewport >= 1220.0 {
            (3, 372.0)
        } else if viewport >= 850.0 {
            (2, 372.0)
        } else if viewport >= 720.0 {
            (2, 335.0)
        } else {
            (1, 335.0)
        };
        Self {
            columns,
            width: if viewport < 480.0 {
                width.min(crate::theme::content_width(viewport))
            } else {
                width
            },
        }
    }
    /// FilmCard's 404:245 aspect ratio.
    pub fn height(self) -> f32 {
        self.width * 245.0 / 404.0
    }
    /// Total row width, including four-pixel column gaps.
    pub fn total_width(self) -> f32 {
        self.width * self.columns as f32 + 4.0 * self.columns.saturating_sub(1) as f32
    }
    /// Fixed virtualization stride including the 36-pixel row gap.
    pub fn stride(self) -> f32 {
        self.height() + 36.0
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_grid_boundary() {
        for (viewport, columns, width) in [
            (479.99, 1, 335.0),
            (480.0, 1, 335.0),
            (480.01, 1, 335.0),
            (719.99, 1, 335.0),
            (720.0, 2, 335.0),
            (720.01, 2, 335.0),
            (849.99, 2, 335.0),
            (850.0, 2, 372.0),
            (850.01, 2, 372.0),
            (1219.99, 2, 372.0),
            (1220.0, 3, 372.0),
            (1220.01, 3, 372.0),
            (1639.99, 3, 372.0),
            (1640.0, 4, 372.0),
            (1640.01, 4, 372.0),
            (320.0, 1, 280.0),
        ] {
            let grid = Grid::at(viewport);
            assert_eq!(grid.columns, columns);
            assert!((grid.width - width).abs() < 0.01, "{viewport}");
        }
    }
}
