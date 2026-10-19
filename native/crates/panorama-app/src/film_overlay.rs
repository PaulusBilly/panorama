use panorama_core::images::DecodedImage;

/// Rasterize Film's CSS linear-over-radial overlay on a blocking worker.
pub fn overlay(width: u32, height: u32) -> DecodedImage {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    let center = (width as f32 * 0.58, height as f32 * 0.48);
    let radius = (center.0.max(width as f32 - center.0).powi(2)
        + center.1.max(height as f32 - center.1).powi(2))
    .sqrt()
        * 0.72;
    for y in 0..height {
        let t = (y as f32 + 0.5) / height as f32;
        let (start, end, offset, span) = if t < 0.28 {
            (0.48, 0.04, 0.0, 0.28)
        } else if t < 0.52 {
            (0.04, 0.16, 0.28, 0.24)
        } else {
            (0.16, 0.92, 0.52, 0.48)
        };
        let linear = start + (end - start) * (t - offset) / span;
        for x in 0..width {
            let distance =
                ((x as f32 + 0.5 - center.0).powi(2) + (y as f32 + 0.5 - center.1).powi(2)).sqrt();
            let radial = 0.2 * (distance / radius).clamp(0.0, 1.0);
            let alpha = linear + radial * (1.0 - linear);
            rgba.extend_from_slice(&[0, 0, 0, (alpha * 255.0).round() as u8]);
        }
    }
    DecodedImage {
        width,
        height,
        rgba,
    }
}
