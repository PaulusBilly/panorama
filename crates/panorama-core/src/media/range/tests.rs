//! Test equivalents from `tests/unit/media-range.test.ts`.
use super::*;
use crate::media::retry::retry_after_deadline;

fn headers(values: &[(&str, &str)]) -> HeaderMap {
    values
        .iter()
        .map(|(key, value)| {
            (
                key.parse::<http::HeaderName>().unwrap(),
                value.parse::<http::HeaderValue>().unwrap(),
            )
        })
        .collect()
}

#[test]
fn accepts_the_exact_interval_including_a_short_final_chunk() {
    assert_eq!(
        validate_media_range(
            &headers(&[("content-range", "bytes 8-9/10"), ("content-length", "2")]),
            8,
            15,
            Some(10)
        )
        .unwrap(),
        MediaRange {
            start: 8,
            end: 9,
            total: 10
        }
    );
}
#[test]
fn rejects_incompatible_headers_j() {
    for values in [
        vec![("content-range", "bytes 0-3/10")],
        vec![("content-range", "bytes 4-6/10")],
        vec![("content-range", "bytes 4-7/11")],
        vec![("content-range", "bytes 4-7/*")],
        vec![("content-range", "bytes 4-7/9007199254740992")],
        vec![("content-range", "bytes 4-7/10"), ("content-length", "5")],
        vec![
            ("content-range", "bytes 4-7/10"),
            ("content-encoding", "gzip"),
        ],
        vec![
            ("content-range", "bytes 4-7/10"),
            ("content-type", "multipart/byteranges"),
        ],
    ] {
        assert!(
            validate_media_range(&headers(&values), 4, 7, Some(10)).is_err(),
            "{values:?}"
        );
    }
}
#[test]
fn prefers_a_strong_etag_and_excludes_weak_tags() {
    assert_eq!(
        media_validator(&headers(&[
            ("etag", "\"v1\""),
            ("last-modified", "Sun, 04 Oct 2026 12:00:00 GMT")
        ])),
        Some("\"v1\"".into())
    );
    assert_eq!(media_validator(&headers(&[("etag", "W/\"v1\"")])), None);
    assert_eq!(
        media_validator(&headers(&[
            ("etag", "W/\"v1\""),
            ("last-modified", "Sun, 04 Oct 2026 12:00:00 GMT")
        ])),
        Some("Sun, 04 Oct 2026 12:00:00 GMT".into())
    );
}
#[test]
fn preserves_long_server_deadlines_and_rejects_malformed_values() {
    assert_eq!(retry_after_deadline(Some("30"), 1000), Some(31000));
    let now = httpdate::parse_http_date("Sun, 04 Oct 2026 12:00:00 GMT")
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert_eq!(
        retry_after_deadline(Some("Sun, 04 Oct 2026 12:01:00 GMT"), now),
        Some(now + 60000)
    );
    assert_eq!(
        retry_after_deadline(Some("Sun, 04 Oct 2026 11:00:00 GMT"), now),
        Some(now)
    );
    for value in [
        None,
        Some("-1"),
        Some("1.2"),
        Some("garbage"),
        Some("9999999999999999999"),
    ] {
        assert_eq!(retry_after_deadline(value, now), None);
    }
}
#[test]
fn rejects_missing_malformed_and_unsafe_intervals() {
    for value in [
        "",
        "bytes -1-2/3",
        "bytes 1-0/3",
        "bytes 1-2/2",
        "bytes 1-2/3x",
        "bytes 1-2/3 ",
        "bytes 1-2/18446744073709551616",
    ] {
        assert!(validate_media_range(&headers(&[("content-range", value)]), 1, 2, None).is_err());
    }
    assert!(validate_media_range(&HeaderMap::new(), 0, 1, None).is_err());
    assert_eq!(retry_after_deadline(Some(" 30 "), 1000), Some(31000));
}

#[test]
fn rejects_non_http_last_modified_values_that_cannot_be_if_range_validators() {
    for value in ["2026-10-04T12:00:00Z", "2026-10-04", "0", "garbage"] {
        assert_eq!(media_validator(&headers(&[("last-modified", value)])), None);
    }
}
