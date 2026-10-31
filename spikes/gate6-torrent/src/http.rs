use std::{io, io::SeekFrom, sync::Arc, sync::OnceLock, time::Instant};

use axum::{
    body::Body,
    http::{HeaderMap, Method, Response, StatusCode, header},
};
use bytes::Bytes;
use futures::stream;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt},
    sync::watch,
};

pub struct Meter {
    pub start: Instant,
    pub first_byte: OnceLock<f64>,
}

pub fn content_type(name: &str) -> Option<&'static str> {
    match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => Some("video/mp4"),
        "mkv" => Some("video/x-matroska"),
        "webm" => Some("video/webm"),
        "avi" => Some("video/x-msvideo"),
        "mov" => Some("video/quicktime"),
        "ts" => Some("video/mp2t"),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
enum Range {
    Full,
    Partial(u64, u64),
    Unsatisfiable,
}

fn parse_range(value: Option<&str>, size: u64) -> Range {
    let Some(value) = value.and_then(|v| v.strip_prefix("bytes=")) else {
        return Range::Full;
    };
    if value.contains(',') {
        return Range::Full;
    }
    let Some((start, end)) = value.split_once('-') else {
        return Range::Full;
    };
    if start.is_empty() {
        return match end.parse::<u64>() {
            Ok(0) => Range::Unsatisfiable,
            Ok(_) if size == 0 => Range::Unsatisfiable,
            Ok(suffix) => Range::Partial(size.saturating_sub(suffix), size - 1),
            Err(_) => Range::Full,
        };
    }
    let Ok(start) = start.parse::<u64>() else {
        return Range::Full;
    };
    let end = if end.is_empty() {
        size.saturating_sub(1)
    } else {
        let Ok(end) = end.parse::<u64>() else {
            return Range::Full;
        };
        if end < start {
            return Range::Full;
        }
        end
    };
    if start >= size {
        Range::Unsatisfiable
    } else {
        Range::Partial(start, end.min(size - 1))
    }
}

pub async fn serve_reader<R>(
    mut reader: R,
    size: u64,
    mime: &'static str,
    method: Method,
    headers: HeaderMap,
    meter: Arc<Meter>,
    stop: watch::Receiver<()>,
) -> io::Result<Response<Body>>
where
    R: AsyncRead + AsyncSeek + Unpin + Send + 'static,
{
    let range = if method == Method::HEAD
        || headers.contains_key(header::IF_RANGE)
        || headers.get_all(header::RANGE).iter().count() > 1
    {
        Range::Full
    } else {
        parse_range(
            headers.get(header::RANGE).and_then(|v| v.to_str().ok()),
            size,
        )
    };
    let mut response = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, mime);
    let (start, length) = match range {
        Range::Unsatisfiable => {
            return Ok(response
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .header(header::CONTENT_LENGTH, 0)
                .body(Body::empty())
                .unwrap());
        }
        Range::Full => (0, size),
        Range::Partial(start, end) => {
            response = response
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
            (start, end - start + 1)
        }
    };
    response = response.header(header::CONTENT_LENGTH, length);
    if method == Method::HEAD || length == 0 {
        return Ok(response.body(Body::empty()).unwrap());
    }
    reader.seek(SeekFrom::Start(start)).await?;
    let chunks = stream::try_unfold(
        (reader.take(length), stop, meter),
        |(mut reader, mut stop, meter)| async move {
            if reader.limit() == 0 {
                return Ok(None);
            }
            let mut buf = vec![0; 64 * 1024];
            let count = tokio::select! {
                _ = stop.changed() => return Ok(None),
                result = tokio::time::timeout(std::time::Duration::from_secs(90), reader.read(&mut buf)) => {
                    result.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "torrent read timed out after 90 s"))??
                }
            };
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "short torrent stream",
                ));
            }
            if meter
                .first_byte
                .set(meter.start.elapsed().as_secs_f64())
                .is_ok()
            {
                println!(
                    "first byte served: {:.3} s from start",
                    meter.first_byte.get().unwrap()
                );
            }
            buf.truncate(count);
            Ok(Some((Bytes::from(buf), (reader, stop, meter))))
        },
    );
    Ok(response.body(Body::from_stream(chunks)).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use std::io::Cursor;

    async fn request(range: Option<&str>, method: Method, size: u64) -> Response<Body> {
        let mut headers = HeaderMap::new();
        if let Some(range) = range {
            headers.insert(header::RANGE, range.parse().unwrap());
        }
        let (sender, stop) = watch::channel(());
        let response = serve_reader(
            Cursor::new(b"0123456789".to_vec()),
            size,
            "video/mp4",
            method,
            headers,
            Arc::new(Meter {
                start: Instant::now(),
                first_byte: OnceLock::new(),
            }),
            stop,
        )
        .await
        .unwrap();
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, 100).await.unwrap();
        drop(sender);
        Response::from_parts(parts, Body::from(bytes))
    }

    #[tokio::test]
    async fn full_and_partial_responses() {
        for (range, status, expected, content_range) in [
            (None, 200, "0123456789", None),
            (Some("bytes=2-5"), 206, "2345", Some("bytes 2-5/10")),
            (Some("bytes=7-"), 206, "789", Some("bytes 7-9/10")),
            (Some("bytes=-3"), 206, "789", Some("bytes 7-9/10")),
            (Some("bytes=8-100"), 206, "89", Some("bytes 8-9/10")),
            (Some("bytes=-100"), 206, "0123456789", Some("bytes 0-9/10")),
            (Some("nonsense"), 200, "0123456789", None),
            (Some("bytes=4-2"), 200, "0123456789", None),
            (Some("bytes=0-1,4-5"), 200, "0123456789", None),
            (Some("bytes=18446744073709551616-"), 200, "0123456789", None),
        ] {
            let response = request(range, Method::GET, 10).await;
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
            assert_eq!(response.headers()[header::CONTENT_TYPE], "video/mp4");
            assert_eq!(
                response.headers()[header::CONTENT_LENGTH],
                expected.len().to_string()
            );
            assert_eq!(
                response
                    .headers()
                    .get(header::CONTENT_RANGE)
                    .map(|v| v.to_str().unwrap()),
                content_range
            );
            assert_eq!(
                to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
                expected.as_bytes()
            );
        }
    }

    #[tokio::test]
    async fn unsatisfiable_and_empty() {
        for (range, size) in [
            ("bytes=10-", 10),
            ("bytes=-0", 10),
            ("bytes=0-", 0),
            ("bytes=-3", 0),
        ] {
            let response = request(Some(range), Method::GET, size).await;
            assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
            assert_eq!(
                response.headers()[header::CONTENT_RANGE],
                format!("bytes */{size}")
            );
            assert_eq!(response.headers()[header::CONTENT_LENGTH], "0");
            assert!(
                to_bytes(response.into_body(), 100)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        let response = request(None, Method::GET, 0).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "0");
    }

    #[tokio::test]
    async fn head_ignores_range_and_reads_no_body() {
        let response = request(Some("bytes=2-5"), Method::HEAD, 10).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "10");
        assert!(!response.headers().contains_key(header::CONTENT_RANGE));
        assert!(
            to_bytes(response.into_body(), 100)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
