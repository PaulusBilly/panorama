use super::*;

fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = tag.to_vec();
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(body);
    if !body.len().is_multiple_of(2) {
        bytes.push(0);
    }
    bytes
}

fn webp(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(4 + chunks.iter().map(Vec::len).sum::<usize>() as u32).to_le_bytes());
    bytes.extend_from_slice(b"WEBP");
    for chunk in chunks {
        bytes.extend_from_slice(chunk);
    }
    bytes
}

fn vp8(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = vec![0x10, 0, 0, 0x9d, 0x01, 0x2a];
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    chunk(b"VP8 ", &bytes)
}

fn vp8l(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = vec![0x2f];
    bytes.extend_from_slice(&((width - 1) | ((height - 1) << 14)).to_le_bytes());
    chunk(b"VP8L", &bytes)
}

#[test]
fn truncated_webp_frames_cannot_hide_oversized_dimensions_behind_canvas() {
    for frame in [vp8(16383, 16383), vp8l(16383, 16383)] {
        let bytes = webp(&[chunk(b"VP8X", &[0; 10]), frame]);
        let mut truncated = bytes.clone();
        truncated[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        truncated[34..38].copy_from_slice(&u32::MAX.to_le_bytes());
        for bytes in [bytes, truncated] {
            let start = std::time::Instant::now();
            assert_eq!(
                super::super::decode::decode(&bytes).unwrap_err(),
                ImageError::TooLarge
            );
            assert!(start.elapsed() < Duration::from_millis(500));
        }
    }
}

#[test]
fn alpha_inherits_checked_canvas_dimensions() {
    let mut canvas = [0; 10];
    canvas[0] = 0x10;
    canvas[4..7].copy_from_slice(&[0xff, 0x3f, 0]);
    let bytes = webp(&[chunk(b"VP8X", &canvas), chunk(b"ALPH", &[1]), vp8(1, 1)]);
    assert_eq!(
        super::super::decode::decode(&bytes).unwrap_err(),
        ImageError::TooLarge
    );
}

#[test]
fn every_webp_frame_must_match_canvas_even_after_first_frame() {
    for frame in [vp8(2, 1), vp8l(2, 1)] {
        let bytes = webp(&[chunk(b"VP8X", &[0; 10]), vp8l(1, 1), frame]);
        assert_eq!(
            super::super::decode::decode(&bytes).unwrap_err(),
            ImageError::TooLarge
        );
    }
}

#[test]
fn animated_webp_is_unsupported() {
    for animation in [chunk(b"ANIM", &[0; 6]), chunk(b"ANMF", &[0; 16])] {
        let bytes = webp(&[chunk(b"VP8X", &[0; 10]), animation]);
        assert_eq!(
            super::super::decode::decode(&bytes).unwrap_err(),
            ImageError::Unsupported
        );
    }
}

#[test]
fn oversized_png_and_jpeg_headers_are_rejected_before_decoder() {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend_from_slice(&4097u32.to_be_bytes());
    png.extend_from_slice(&1u32.to_be_bytes());
    let jpeg = vec![0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 1, 0x10, 1];
    for bytes in [png, jpeg] {
        assert_eq!(
            super::super::decode::decode(&bytes).unwrap_err(),
            ImageError::TooLarge
        );
    }
}

#[test]
fn jpeg_checks_sof_headers_after_entropy_coded_scans() {
    let mut bytes = vec![0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 1, 0, 1, 1, 1, 0x11, 0];
    bytes.extend_from_slice(&[0xff, 0xda, 0, 8, 1, 1, 0, 0, 63, 0]);
    bytes.extend_from_slice(&[1, 0xff, 0, 0xc0, 0xff, 0xd0]);
    bytes.extend_from_slice(&[0xff, 0xc0, 0, 17, 8, 0, 1, 0x10, 1]);
    assert_eq!(
        super::super::decode::decode(&bytes).unwrap_err(),
        ImageError::TooLarge
    );
}
