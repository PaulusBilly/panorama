use std::{io::Cursor, sync::Arc};

use image::{DynamicImage, ImageFormat, ImageReader, imageops::FilterType};

use super::{DecodedImage, ImageError, ImageTarget};

pub(super) fn decode(bytes: &[u8]) -> Result<DynamicImage, ImageError> {
    let format = image::guess_format(bytes).map_err(|_| ImageError::Unsupported)?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
    ) {
        return Err(ImageError::Unsupported);
    }
    super::headers::check(bytes, format)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|error| match error {
        image::ImageError::Limits(_) => ImageError::TooLarge,
        _ => ImageError::Decode,
    })
}

pub(super) fn resize(image: Arc<DynamicImage>, target: ImageTarget) -> DecodedImage {
    let rgba = if image.width() <= target.max_width && image.height() <= target.max_height {
        image.to_rgba8()
    } else {
        image
            .resize(target.max_width, target.max_height, FilterType::Triangle)
            .into_rgba8()
    };
    DecodedImage {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    }
}
