use image::ImageFormat;

use super::ImageError;

pub(super) fn check(bytes: &[u8], format: ImageFormat) -> Result<(), ImageError> {
    match format {
        ImageFormat::Png => png(bytes),
        ImageFormat::Jpeg => jpeg(bytes),
        ImageFormat::WebP => webp(bytes),
        _ => Err(ImageError::Unsupported),
    }
}

fn dimensions(width: u32, height: u32) -> Result<(u32, u32), ImageError> {
    if width > 4096 || height > 4096 {
        return Err(ImageError::TooLarge);
    }
    if width == 0 || height == 0 {
        return Err(ImageError::Decode);
    }
    Ok((width, height))
}

fn png(bytes: &[u8]) -> Result<(), ImageError> {
    if bytes.get(8..16) != Some(b"\0\0\0\rIHDR") {
        return Err(ImageError::Decode);
    }
    let header = bytes.get(16..24).ok_or(ImageError::Decode)?;
    let width = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let height = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    dimensions(width, height)?;
    Ok(())
}

fn jpeg(bytes: &[u8]) -> Result<(), ImageError> {
    let mut offset = 2;
    let mut found = false;
    let mut in_scan = false;
    while offset < bytes.len() {
        if in_scan {
            while offset < bytes.len() && bytes[offset] != 0xff {
                offset += 1;
            }
            if offset == bytes.len() {
                break;
            }
        }
        if bytes[offset] != 0xff {
            return Err(ImageError::Decode);
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes.get(offset).ok_or(ImageError::Decode)?;
        offset += 1;
        if marker == 0xd9 {
            break;
        }
        if in_scan && marker == 0 {
            continue;
        }
        if matches!(marker, 0x01 | 0xd0..=0xd8) {
            continue;
        }
        in_scan = marker == 0xda;
        let length = bytes.get(offset..offset + 2).ok_or(ImageError::Decode)?;
        let length = usize::from(u16::from_be_bytes([length[0], length[1]]));
        if length < 2 {
            return Err(ImageError::Decode);
        }
        if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            if length < 7 {
                return Err(ImageError::Decode);
            }
            let header = bytes
                .get(offset + 2..offset + 7)
                .ok_or(ImageError::Decode)?;
            let height = u32::from(u16::from_be_bytes([header[1], header[2]]));
            let width = u32::from(u16::from_be_bytes([header[3], header[4]]));
            dimensions(width, height)?;
            found = true;
        }
        offset = offset.checked_add(length).ok_or(ImageError::Decode)?;
        if offset > bytes.len() {
            return Err(ImageError::Decode);
        }
    }
    if found {
        Ok(())
    } else {
        Err(ImageError::Decode)
    }
}

fn le24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn webp(bytes: &[u8]) -> Result<(), ImageError> {
    let header = bytes.get(..12).ok_or(ImageError::Decode)?;
    let declared_end = u64::from(le32(&header[4..8])) + 8;
    let end = bytes
        .len()
        .min(usize::try_from(declared_end).map_err(|_| ImageError::Decode)?);
    let mut offset = 12;
    let mut canvas = None;
    let mut frame = None;
    while offset < end {
        let header = bytes
            .get(offset..offset + 8)
            .filter(|_| offset + 8 <= end)
            .ok_or(ImageError::Decode)?;
        let size = usize::try_from(le32(&header[4..8])).map_err(|_| ImageError::Decode)?;
        let start = offset + 8;
        let next = start.checked_add(size).ok_or(ImageError::Decode)?;
        let body = &bytes[start..next.min(end)];
        let frame_dimensions = match &header[..4] {
            b"ANIM" | b"ANMF" => return Err(ImageError::Unsupported),
            b"VP8X" => {
                let header = body.get(..10).ok_or(ImageError::Decode)?;
                let dims = dimensions(le24(&header[4..7]) + 1, le24(&header[7..10]) + 1)?;
                if header[0] & 2 != 0 {
                    return Err(ImageError::Unsupported);
                }
                if offset != 12 || size != 10 {
                    return Err(ImageError::Decode);
                }
                canvas = Some(dims);
                None
            }
            b"VP8 " => {
                let header = body.get(..10).ok_or(ImageError::Decode)?;
                if header[0] & 1 != 0 || header[3..6] != [0x9d, 1, 0x2a] {
                    return Err(ImageError::Decode);
                }
                let width = u16::from_le_bytes([header[6], header[7]]) & 0x3fff;
                let height = u16::from_le_bytes([header[8], header[9]]) & 0x3fff;
                Some(dimensions(u32::from(width), u32::from(height))?)
            }
            b"VP8L" => {
                let header = body.get(..5).ok_or(ImageError::Decode)?;
                if header[0] != 0x2f {
                    return Err(ImageError::Decode);
                }
                let bits = le32(&header[1..5]);
                Some(dimensions(
                    (bits & 0x3fff) + 1,
                    ((bits >> 14) & 0x3fff) + 1,
                )?)
            }
            b"ALPH" => {
                let info = *body.first().ok_or(ImageError::Decode)?;
                let (width, height) = canvas.ok_or(ImageError::Decode)?;
                dimensions(width, height)?;
                if info & 3 > 1 || (info >> 4) & 3 > 1 || info & 0xc0 != 0 {
                    return Err(ImageError::Decode);
                }
                None
            }
            _ => None,
        };
        if let Some(dims) = frame_dimensions {
            if canvas.or(frame).is_some_and(|expected| expected != dims) {
                return Err(ImageError::TooLarge);
            }
            frame = Some(dims);
        }
        offset = next.checked_add(size & 1).ok_or(ImageError::Decode)?;
        if offset > end {
            return Err(ImageError::Decode);
        }
    }
    if declared_end != bytes.len() as u64 || frame.is_none() {
        return Err(ImageError::Decode);
    }
    Ok(())
}
