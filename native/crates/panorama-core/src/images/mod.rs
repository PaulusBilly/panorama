//! HTTPS image loading with bounded decoding and an original-byte disk cache.
//!
//! Share a loader inside a Tokio runtime. Loads perform disk I/O, decoding, and
//! resizing on blocking workers. Construct the loader off the UI thread too:
//! construction rebuilds and prunes the disk index synchronously.
//!
//! Originals live at `<cache>/<first two SHA-256 hex digits>/<SHA-256 hex>`.
//! A persistent sibling `<cache>.images.lock` coordinates cooperating processes.
//! Readers hold this lock until their file handles close, so Windows eviction and
//! clearing wait for readers instead of deleting an open file. Atomic renames
//! publish complete files; abandoned temporary files are removed under the lock.
//! Modified times are touched on reads and determine the LRU order after restart.
//! An overflow evicts oldest originals until at most 90% of the budget remains.

mod cache;
mod decode;
mod destination;
mod headers;
mod loader;

pub use loader::ImageLoader;

use std::{error::Error, fmt, path::PathBuf, time::Duration};

/// A normalized HTTPS URL. Debug output deliberately hides its contents.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct ImageUrl(reqwest::Url);

impl ImageUrl {
    /// Validates HTTPS, a host, no userinfo, and at most 2,048 characters.
    /// Fragments are removed because they do not identify an HTTP resource.
    pub fn parse(input: &str) -> Result<Self, ImageError> {
        if input.chars().count() > 2048 || input.chars().any(char::is_control) {
            return Err(ImageError::InvalidUrl);
        }
        let mut url = reqwest::Url::parse(input).map_err(|_| ImageError::InvalidUrl)?;
        if !valid_url(&url)
            || input.split_once("://").is_some_and(|(_, rest)| {
                rest.split(['/', '?', '#'])
                    .next()
                    .is_some_and(|s| s.contains('@'))
            })
        {
            return Err(ImageError::InvalidUrl);
        }
        url.set_fragment(None);
        Ok(Self(url))
    }
}

fn valid_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some_and(|host| !host.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.as_str().chars().count() <= 2048
        && url.host_str().is_some_and(|host| {
            match host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
                Ok(address) => !destination::forbidden_address(address),
                Err(_) => true,
            }
        })
}

impl fmt::Debug for ImageUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImageUrl([redacted])")
    }
}

/// Maximum output dimensions. Zero dimensions are rejected as `Decode`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageTarget {
    /// Maximum output width in pixels.
    pub max_width: u32,
    /// Maximum output height in pixels.
    pub max_height: u32,
}

/// An image resource and its desired output bounds.
#[derive(Clone, Debug)]
pub struct ImageRequest {
    /// Validated image resource.
    pub url: ImageUrl,
    /// Aspect-preserving bounds; images are never upscaled.
    pub target: ImageTarget,
}

/// Decoded straight (unpremultiplied) RGBA8 pixels, in row-major order.
/// The GPUI adapter must convert RGBA to BGRA when creating a `RenderImage`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedImage {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Exactly `width * height * 4` bytes, in RGBA channel order.
    pub rgba: Vec<u8>,
}

/// Disk, transport, and scheduling limits for an image loader.
#[derive(Clone, Debug)]
pub struct ImageLoaderOptions {
    /// Dedicated cache directory; defaults to `panorama-images` relative to cwd.
    pub cache_dir: PathBuf,
    /// Maximum encoded cache bytes; default 256 MiB. Zero disables retention.
    pub max_disk_bytes: u64,
    /// Maximum encoded response bytes; default 8 MiB.
    pub max_body_bytes: u64,
    /// Maximum simultaneous downloads; default six. Must be nonzero.
    pub max_concurrent: usize,
    /// Per-download timeout, including the streamed body; default 20 seconds.
    pub timeout: Duration,
}

impl Default for ImageLoaderOptions {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("panorama-images"),
            max_disk_bytes: 256 * 1024 * 1024,
            max_body_bytes: 8 * 1024 * 1024,
            max_concurrent: 6,
            timeout: Duration::from_secs(20),
        }
    }
}

/// Sanitized failures. No URL, response body, or underlying diagnostic is retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageError {
    /// The resource URL violates the HTTPS URL policy.
    InvalidUrl,
    /// Transport, HTTP status, or redirect policy failure.
    Network,
    /// The download exceeded its timeout.
    Timeout,
    /// Encoded bytes or decoder resource limits were exceeded.
    TooLarge,
    /// Content sniffing found a format other than JPEG, PNG, or WebP.
    Unsupported,
    /// The image is malformed, the target is invalid, or a decode worker failed.
    Decode,
    /// Cache configuration, filesystem, or worker failure.
    Cache,
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidUrl => "invalid image URL",
            Self::Network => "image network failure",
            Self::Timeout => "image download timed out",
            Self::TooLarge => "image exceeds size limits",
            Self::Unsupported => "unsupported image format",
            Self::Decode => "image decoding failed",
            Self::Cache => "image cache failure",
        })
    }
}

impl Error for ImageError {}

#[cfg(test)]
mod tests;
