//! Representation validation ported from `desktop/main/media-range.ts`.

use super::MediaError;
use http::HeaderMap;

pub(crate) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Exact upstream interval and total representation size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaRange {
    /// Inclusive first byte.
    pub start: u64,
    /// Inclusive last byte.
    pub end: u64,
    /// Total file size.
    pub total: u64,
}

pub(crate) fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn decimal(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Validates exact Content-Range, identity encoding, type and optional length.
pub fn validate_media_range(
    headers: &HeaderMap,
    expected_start: u64,
    expected_end: u64,
    expected_total: Option<u64>,
) -> Result<MediaRange, MediaError> {
    let invalid = MediaError::Representation("Invalid media range");
    let raw = header(headers, "content-range")
        .and_then(|value| value.strip_prefix("bytes "))
        .ok_or(invalid.clone())?;
    let (interval, total) = raw.split_once('/').ok_or(invalid.clone())?;
    let (start, end) = interval.split_once('-').ok_or(invalid.clone())?;
    let (start, end, total) = (
        decimal(start).ok_or(invalid.clone())?,
        decimal(end).ok_or(invalid.clone())?,
        decimal(total).ok_or(invalid)?,
    );
    if [start, end, total, expected_start, expected_end]
        .iter()
        .any(|value| *value > MAX_SAFE_INTEGER)
        || end < start
        || total <= end
        || start != expected_start
        || end != expected_end.min(total - 1)
        || expected_total.is_some_and(|value| value != total)
    {
        return Err(MediaError::Representation("Media range changed"));
    }
    if header(headers, "content-encoding")
        .is_some_and(|value| !value.is_empty() && value != "identity")
    {
        return Err(MediaError::Representation("Encoded media range"));
    }
    if header(headers, "content-type")
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("multipart/"))
    {
        return Err(MediaError::Representation("Multipart media range"));
    }
    if header(headers, "content-length")
        .is_some_and(|value| decimal(value) != Some(end - start + 1))
    {
        return Err(MediaError::Representation("Invalid media range length"));
    }
    Ok(MediaRange { start, end, total })
}

/// Prefers a strong ETag, falling back to a parseable Last-Modified date.
pub fn media_validator(headers: &HeaderMap) -> Option<String> {
    if let Some(etag) = header(headers, "etag")
        && etag.len() >= 2
        && etag.starts_with('"')
        && etag.ends_with('"')
        && !etag[1..etag.len() - 1].contains(['"', '\r', '\n'])
    {
        return Some(etag.to_owned());
    }
    header(headers, "last-modified")
        .filter(|value| httpdate::parse_http_date(value).is_ok())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
