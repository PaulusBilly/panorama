#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SurfaceRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn physical_rect(width: f32, height: f32, scale: f32, fullscreen: bool) -> SurfaceRect {
    if !width.is_finite() || !height.is_finite() || !scale.is_finite() || scale <= 0.0 {
        return SurfaceRect::default();
    }
    // macOS draws the window's title bar transparently over the content.
    let top = if fullscreen || cfg!(target_os = "macos") {
        0.0
    } else {
        32.0_f32.min(height.max(0.0))
    };
    let y = (top * scale).floor() as i32;
    SurfaceRect {
        x: 0,
        y,
        width: (width.max(0.0) * scale).ceil() as i32,
        height: ((height.max(0.0) * scale).ceil() as i32).saturating_sub(y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outward_rounding_titlebar_and_fullscreen_at_all_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for fullscreen in [false, true] {
                let rect = physical_rect(1024.1, 700.2, scale, fullscreen);
                let top = if fullscreen || cfg!(target_os = "macos") {
                    0.0
                } else {
                    32.0 * scale
                };
                assert_eq!(rect.y, top.floor() as i32);
                assert_eq!(rect.width, (1024.1 * scale).ceil() as i32);
                assert_eq!(rect.y + rect.height, (700.2 * scale).ceil() as i32);
                assert!(rect.width as f32 >= 1024.1 * scale);
            }
        }
        assert_eq!(physical_rect(0.0, 0.0, 1.0, false), SurfaceRect::default());
        assert_eq!(
            physical_rect(f32::NAN, 10.0, 1.0, false),
            SurfaceRect::default()
        );
    }
}
