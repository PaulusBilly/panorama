use super::{TorrentError, lock, runtime::Shared};
use bytes::Bytes;
use http::{Method, Request, Response, header};
use http_body_util::{BodyExt, Full, StreamBody, combinators::UnsyncBoxBody};
use hyper::{
    body::{Frame, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{convert::Infallible, io::SeekFrom, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt},
    net::TcpListener,
    task::JoinSet,
};

type Body = UnsyncBoxBody<Bytes, TorrentError>;
const DEADLINE: Duration = Duration::from_millis(300);

pub(super) async fn accept(listener: TcpListener, shared: Arc<Shared>) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = shared.stop.cancelled() => break,
            _ = connections.join_next(), if !connections.is_empty() => {},
            result = listener.accept() => {
                let (socket, peer) = match result {
                    Ok(result) => result,
                    Err(_) => {
                        tokio::select! {
                            _ = shared.stop.cancelled() => break,
                            _ = tokio::time::sleep(Duration::from_millis(50)) => {},
                        }
                        continue;
                    }
                };
                let shared = shared.clone();
                connections.spawn(async move {
                    let state = shared.clone();
                    let (lease_tx, mut leases) = tokio::sync::watch::channel::<Option<tokio_util::sync::CancellationToken>>(None);
                    let service = service_fn(move |request| {
                        let state = state.clone();
                        let lease_tx = lease_tx.clone();
                        async move { Ok::<_, Infallible>(serve(request, peer, state, lease_tx).await) }
                    });
                    let connection = http1::Builder::new().serve_connection(TokioIo::new(socket), service);
                    tokio::pin!(connection);
                    loop {
                        let lease = leases.borrow().clone().unwrap_or_default();
                        let stopping = tokio::select! {
                            biased;
                            _ = &mut connection => break,
                            _ = shared.stop.cancelled() => true,
                            _ = lease.cancelled() => true,
                            _ = leases.changed() => false,
                        };
                        if stopping {
                            connection.as_mut().graceful_shutdown();
                            let _ = tokio::time::timeout(DEADLINE, &mut connection).await;
                            break;
                        }
                    }
                });
            }
        }
    }
    drop(listener);
    if tokio::time::timeout(DEADLINE, async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }
}

pub(super) fn authorized(
    headers: &http::HeaderMap,
    uri: &http::Uri,
    peer: SocketAddr,
    host: &str,
) -> bool {
    peer.ip().is_loopback()
        && headers.get_all(header::HOST).iter().count() == 1
        && headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            == Some(host)
        && uri.scheme().is_none()
        && uri.authority().is_none()
}

fn empty(status: u16) -> Response<Body> {
    let mut response = Response::new(
        Full::new(Bytes::new())
            .map_err(|never: Infallible| match never {})
            .boxed_unsync(),
    );
    if let Ok(status) = http::StatusCode::from_u16(status) {
        *response.status_mut() = status;
    }
    response
}

async fn serve(
    request: Request<Incoming>,
    peer: SocketAddr,
    shared: Arc<Shared>,
    lease: tokio::sync::watch::Sender<Option<tokio_util::sync::CancellationToken>>,
) -> Response<Body> {
    if !authorized(request.headers(), request.uri(), peer, &shared.host)
        || !matches!(*request.method(), Method::GET | Method::HEAD)
    {
        return empty(404);
    }
    let path = request.uri().to_string();
    let playback = path
        .strip_prefix("/torrent/")
        .filter(|token| {
            token.len() == 32
                && token
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .and_then(|token| lock(&shared.routes).get(token).cloned());
    let Some(playback) = playback else {
        return empty(404);
    };
    lease.send_replace(Some(playback.stop.clone()));
    if playback.stop.is_cancelled() {
        return empty(410);
    }
    if playback.entry.snapshot().error.is_some() {
        return empty(503);
    }
    let range = if request.method() == Method::HEAD
        || request.headers().contains_key(header::IF_RANGE)
        || request.headers().get_all(header::RANGE).iter().count() > 1
    {
        None
    } else {
        request
            .headers()
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok())
    };
    let (start, length, ranged) = match client_range(range, playback.size) {
        Ok(range) => range,
        Err(()) => {
            let mut response = empty(416);
            insert(
                &mut response,
                header::CONTENT_RANGE,
                format!("bytes */{}", playback.size),
            );
            insert(&mut response, header::CONTENT_LENGTH, "0".into());
            return response;
        }
    };
    let mut response = empty(if ranged { 206 } else { 200 });
    insert(&mut response, header::ACCEPT_RANGES, "bytes".into());
    insert(
        &mut response,
        header::CONTENT_TYPE,
        "application/octet-stream".into(),
    );
    insert(&mut response, header::CONTENT_LENGTH, length.to_string());
    if ranged {
        insert(
            &mut response,
            header::CONTENT_RANGE,
            format!(
                "bytes {start}-{}/{size}",
                start + length - 1,
                size = playback.size
            ),
        );
    }
    if request.method() == Method::HEAD || length == 0 {
        return response;
    }
    let reader = tokio::select! {
        _ = playback.stop.cancelled() => return empty(410),
        result = tokio::time::timeout(shared.options.no_peers_timeout, playback.entry.reader(playback.index)) => result,
    };
    let mut reader = match reader {
        Ok(Ok(reader)) => reader,
        _ => return empty(503),
    };
    if reader.inner.seek(SeekFrom::Start(start)).await.is_err() {
        return empty(503);
    }
    let stream = futures::stream::try_unfold(
        (reader, length, playback, shared.options.no_peers_timeout),
        |(mut reader, remaining, playback, deadline)| async move {
            if remaining == 0 {
                return Ok(None);
            }
            if let Some(error) = playback.entry.snapshot().error {
                return Err(error);
            }
            let mut buf = vec![0; remaining.min(64 * 1024) as usize];
            let count = tokio::select! {
                _ = playback.stop.cancelled() => return Err(TorrentError::Cancelled),
                result = tokio::time::timeout(deadline, reader.inner.read(&mut buf)) => {
                    result.map_err(|_| {
                        let error = playback.entry.snapshot().error.unwrap_or(TorrentError::NoPeers);
                        playback.entry.fail(error);
                        error
                    })?.map_err(|_| {
                        let error = playback.entry.snapshot().error.unwrap_or(TorrentError::Engine);
                        playback.entry.fail(error);
                        error
                    })?
                }
            };
            if count == 0 {
                return Err(TorrentError::Engine);
            }
            buf.truncate(count);
            Ok(Some((
                Frame::data(Bytes::from(buf)),
                (reader, remaining - count as u64, playback, deadline),
            )))
        },
    );
    *response.body_mut() = StreamBody::new(stream).boxed_unsync();
    response
}

fn insert(response: &mut Response<Body>, name: header::HeaderName, value: String) {
    if let Ok(value) = value.parse() {
        response.headers_mut().insert(name, value);
    }
}

pub(super) fn client_range(value: Option<&str>, size: u64) -> Result<(u64, u64, bool), ()> {
    let Some(value) = value.and_then(|value| value.strip_prefix("bytes=")) else {
        return Ok((0, size, false));
    };
    if value.contains(',') {
        return Ok((0, size, false));
    }
    let Some((left, right)) = value.split_once('-') else {
        return Ok((0, size, false));
    };
    if !left.bytes().all(|byte| byte.is_ascii_digit())
        || !right.bytes().all(|byte| byte.is_ascii_digit())
        || (left.is_empty() && right.is_empty())
    {
        return Ok((0, size, false));
    }
    if size == 0 {
        return Err(());
    }
    let (start, end) = if left.is_empty() {
        let suffix = right.parse::<u64>().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        (size.saturating_sub(suffix), size - 1)
    } else {
        let start = left.parse::<u64>().map_err(|_| ())?;
        if start >= size {
            return Err(());
        }
        let end = if right.is_empty() {
            size - 1
        } else {
            right.parse::<u64>().map_err(|_| ())?.min(size - 1)
        };
        if end < start {
            return Ok((0, size, false));
        }
        (start, end)
    };
    if start >= size {
        return Err(());
    }
    Ok((start, end - start + 1, true))
}
