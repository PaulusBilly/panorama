//! HTTP serving and peer validation ported from `desktop/main/media-proxy.ts`.

use super::{
    Shared,
    session::{Mode, Session, cancelled},
};
use crate::media::{
    MediaError,
    fetch::FetchRequest,
    lock,
    range::{MAX_SAFE_INTEGER, header},
};
use bytes::Bytes;
use futures::StreamExt;
use http::{Request, Response};
use http_body_util::{BodyExt, Full, StreamBody, combinators::UnsyncBoxBody};
use hyper::{
    body::{Frame, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, task::JoinSet};

type Body = UnsyncBoxBody<Bytes, MediaError>;
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

pub(super) async fn accept(listener: TcpListener, shared: Arc<Shared>, deadline: Duration) {
    let listener = Arc::new(listener);
    accept_with(
        move || {
            let listener = listener.clone();
            async move { listener.accept().await }
        },
        shared,
        deadline,
    )
    .await;
}

async fn accept_with<F, Fut>(mut next: F, shared: Arc<Shared>, deadline: Duration)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::io::Result<(tokio::net::TcpStream, SocketAddr)>>,
{
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = cancelled(shared.stop.subscribe()) => break,
            _ = connections.join_next(), if !connections.is_empty() => {},
            result = next() => {
                let (socket, peer) = match result {
                    Ok(connection) => connection,
                    Err(_) => {
                        tokio::select! {
                            biased;
                            _ = cancelled(shared.stop.subscribe()) => break,
                            _ = tokio::time::sleep(Duration::from_millis(50)) => {},
                        }
                        continue;
                    }
                };
                let shared = shared.clone();
                connections.spawn(async move {
                    let state = shared.clone();
                    let service = service_fn(move |request| { let state = state.clone(); async move { Ok::<_, Infallible>(serve(request, peer, state).await) } });
                    let connection = http1::Builder::new().serve_connection(TokioIo::new(socket), service);
                    tokio::pin!(connection);
                    tokio::select! {
                        _ = &mut connection => {},
                        _ = cancelled(shared.stop.subscribe()) => {
                            connection.as_mut().graceful_shutdown();
                            let _ = tokio::time::timeout(deadline, &mut connection).await;
                        }
                    }
                });
            }
        }
    }
    drop(next);
    let drained = tokio::time::timeout(deadline, async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }
}

#[cfg(test)]
mod tests;

pub(super) fn authorized(
    headers: &http::HeaderMap,
    uri: &http::Uri,
    peer: SocketAddr,
    host: &str,
) -> bool {
    peer.ip().is_loopback()
        && header(headers, "host") == Some(host)
        && uri.scheme().is_none()
        && uri.authority().is_none()
}

async fn serve(
    request: Request<Incoming>,
    peer: SocketAddr,
    shared: Arc<Shared>,
) -> Response<Body> {
    if !authorized(request.headers(), request.uri(), peer, &shared.host) {
        return empty(404);
    }
    if request.method() != http::Method::GET && request.method() != http::Method::HEAD {
        return empty(404);
    }
    let path = request.uri().to_string();
    let token = path.strip_prefix("/media/").filter(|token| {
        token.len() == 32
            && token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    let session = token.and_then(|token| lock(&shared.sessions).get(token).cloned());
    let Some(session) = session else {
        return empty(404);
    };
    if session.stopped() {
        return empty(if lock(&session.state).error.is_some() {
            502
        } else {
            404
        });
    }
    if lock(&session.state).readers.len() >= 8 {
        return empty(503);
    }
    let mode = match session.prepare().await {
        Ok(mode) => mode,
        Err(_) => return empty(502),
    };
    if mode == Mode::Passthrough {
        return passthrough(request, session).await;
    }
    let (size, content_type) = {
        let state = lock(&session.state);
        (state.size.unwrap_or(0), state.content_type.clone())
    };
    let range = client_range(header(request.headers(), "range"), size);
    let (start, end, ranged) = match range {
        Ok(range) => range,
        Err(()) => {
            let mut response = empty(416);
            if let Ok(value) = http::HeaderValue::from_str(&format!("bytes */{size}")) {
                response.headers_mut().insert("content-range", value);
            }
            return response;
        }
    };
    let mut response = empty(if ranged { 206 } else { 200 });
    for (name, value) in [
        ("accept-ranges", "bytes".into()),
        ("content-length", (end - start + 1).to_string()),
        ("content-type", content_type),
    ] {
        if let Ok(value) = http::HeaderValue::from_str(&value) {
            response.headers_mut().insert(name, value);
        }
    }
    if ranged && let Ok(value) = http::HeaderValue::from_str(&format!("bytes {start}-{end}/{size}"))
    {
        response.headers_mut().insert("content-range", value);
    }
    if request.method() == http::Method::HEAD {
        return response;
    }
    let reader = match session.add_reader(start) {
        Ok(reader) => reader,
        Err(_) => return empty(503),
    };
    let stream = futures::stream::unfold(Some(reader), move |reader| async move {
        let mut reader = reader?;
        match reader.read(end).await {
            Ok(Some(bytes)) => Some((Ok::<_, MediaError>(Frame::data(bytes)), Some(reader))),
            Ok(None) => None,
            Err(error) => Some((Err(error), None)),
        }
    });
    *response.body_mut() = StreamBody::new(stream).boxed_unsync();
    response
}

pub(super) fn client_range(value: Option<&str>, size: u64) -> Result<(u64, u64, bool), ()> {
    if size == 0 {
        return Err(());
    }
    let parsed = value
        .and_then(|value| value.strip_prefix("bytes="))
        .and_then(|value| value.split_once('-'))
        .filter(|(start, end)| {
            start.bytes().all(|byte| byte.is_ascii_digit())
                && end.bytes().all(|byte| byte.is_ascii_digit())
        });
    let (mut start, mut end) = (0, size - 1);
    if let Some((left, right)) = parsed {
        if !left.is_empty() {
            start = left.parse::<u64>().map_err(|_| ())?;
            if !right.is_empty() {
                end = right.parse::<u64>().map_err(|_| ())?.min(size - 1);
            }
        } else if !right.is_empty() {
            start = size.saturating_sub(right.parse::<u64>().map_err(|_| ())?);
        }
    }
    if start > MAX_SAFE_INTEGER || end > MAX_SAFE_INTEGER || start >= size || start > end {
        return Err(());
    }
    Ok((start, end, parsed.is_some()))
}

async fn passthrough(request: Request<Incoming>, session: Arc<Session>) -> Response<Body> {
    let mut headers = http::HeaderMap::new();
    if let Some(range) = request.headers().get("range") {
        headers.insert("range", range.clone());
    }
    let upstream = tokio::select! {
        _ = cancelled(session.stop_signal.subscribe()) => return empty(410),
        result = session.fetch.fetch(session.original.clone(), FetchRequest { method: request.method().clone(), headers }) => match result { Ok(result) => result, Err(_) => return empty(502) },
    };
    let mut response = empty(upstream.status);
    for name in [
        "content-type",
        "content-length",
        "content-range",
        "accept-ranges",
    ] {
        if let Some(value) = upstream.headers.get(name) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    if request.method() != http::Method::HEAD {
        let body = upstream
            .body
            .take_until(cancelled(session.stop_signal.subscribe()))
            .map(|part| part.map(Frame::data));
        *response.body_mut() = StreamBody::new(body).boxed_unsync();
    }
    response
}
