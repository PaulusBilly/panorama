mod cache;
mod coordination;
mod decode;
mod destination;
mod fetch;
mod process;
mod server;

use std::{io::Cursor, path::Path, time::Duration};

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

use super::*;

fn encoded(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
    let rgba = RgbaImage::from_pixel(width, height, Rgba([210, 30, 60, 255]));
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(rgba)
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

fn options(root: &Path) -> ImageLoaderOptions {
    ImageLoaderOptions {
        cache_dir: root.join("images"),
        ..Default::default()
    }
}

fn request(server: &server::Server, path: &str, width: u32, height: u32) -> ImageRequest {
    ImageRequest {
        url: ImageUrl::parse(&server.url(path)).unwrap(),
        target: ImageTarget {
            max_width: width,
            max_height: height,
        },
    }
}

fn assert_error(error: ImageError, expected: ImageError, url: &str) {
    assert_eq!(error, expected);
    assert!(!format!("{error:?} {error}").contains(url));
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn urls_validate_and_normalize_without_exposing_metadata() {
    for url in [
        "http://example.test/a",
        "data:image/png;base64,abc",
        "file:///a",
        "https://user@example.test/a",
        "https://u:p@example.test",
        "https://@example.test",
        "https://",
        "https:///",
        "https://example.test/\n",
    ] {
        assert_error(
            ImageUrl::parse(url).unwrap_err(),
            ImageError::InvalidUrl,
            url,
        );
    }
    let long = format!("https://example.test/{}", "a".repeat(2048));
    assert_error(
        ImageUrl::parse(&long).unwrap_err(),
        ImageError::InvalidUrl,
        &long,
    );
    let first = ImageUrl::parse("https://EXAMPLE.test:443/a#secret").unwrap();
    let second = ImageUrl::parse("https://example.test/a").unwrap();
    assert_eq!(first, second);
    assert!(!format!("{first:?}").contains("example.test"));
    let boundary = format!("https://example.test/{}", "a".repeat(2048 - 21));
    assert_eq!(boundary.chars().count(), 2048);
    assert!(ImageUrl::parse(&boundary).is_ok());
}

#[test]
fn every_error_and_request_debug_is_sanitized() {
    let url = "https://example.test/private?token=secret";
    for error in [
        ImageError::InvalidUrl,
        ImageError::Network,
        ImageError::Timeout,
        ImageError::TooLarge,
        ImageError::Unsupported,
        ImageError::Decode,
        ImageError::Cache,
    ] {
        assert_error(error, error, url);
        assert!(!format!("{error:?} {error}").contains("secret"));
    }
    let request = ImageRequest {
        url: ImageUrl::parse(url).unwrap(),
        target: ImageTarget {
            max_width: 1,
            max_height: 1,
        },
    };
    assert!(!format!("{request:?}").contains("secret"));
}

#[test]
fn defaults_and_configuration_errors() {
    let defaults = ImageLoaderOptions::default();
    assert_eq!(defaults.max_disk_bytes, 256 * 1024 * 1024);
    assert_eq!(defaults.max_body_bytes, 8 * 1024 * 1024);
    assert_eq!(defaults.max_concurrent, 6);
    assert_eq!(defaults.timeout, Duration::from_secs(20));
    let dir = tempfile::tempdir().unwrap();
    let mut options = options(dir.path());
    options.max_concurrent = 0;
    assert!(matches!(ImageLoader::new(options), Err(ImageError::Cache)));
    let file = dir.path().join("file");
    std::fs::write(&file, b"occupied").unwrap();
    let mut options = self::options(dir.path());
    options.cache_dir = file;
    assert!(matches!(ImageLoader::new(options), Err(ImageError::Cache)));
}
