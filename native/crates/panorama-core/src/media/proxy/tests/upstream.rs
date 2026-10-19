//! Real Hyper upstream and reqwest redirect tests for `media-fetch.ts` and `media-proxy.ts`.
use super::*;
use crate::media::fetch::{FetchRequest, HttpMediaFetch, MediaFetch};
use http_body_util::{BodyExt, Full, StreamBody, combinators::UnsyncBoxBody};
use hyper::{
    body::{Frame, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{
    convert::Infallible,
    sync::atomic::{AtomicBool, AtomicUsize},
};
use tokio::{
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};

type Body = UnsyncBoxBody<Bytes, MediaError>;
struct Origin {
    source: String,
    address: std::net::SocketAddr,
    task: JoinHandle<()>,
    peak: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<(u64, tokio::time::Instant)>>>,
    resolver_hits: Arc<AtomicUsize>,
}
impl Drop for Origin {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct Active(Arc<AtomicUsize>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
struct CheckedFetch {
    transport: HttpMediaFetch,
    session: Arc<Mutex<Option<std::sync::Weak<session::Session>>>>,
}
impl MediaFetch for CheckedFetch {
    fn fetch(
        &self,
        source: MediaSource,
        request: FetchRequest,
    ) -> futures::future::BoxFuture<'static, Result<crate::media::fetch::FetchResponse, MediaError>>
    {
        if crate::media::range::header(&request.headers, "range") != Some("bytes=0-0")
            && let Some(session) = lock(&self.session)
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
        {
            let state = lock(&session.state);
            assert!(state.active <= state.parallel());
        }
        self.transport.fetch(source, request)
    }
}
fn full(bytes: Bytes) -> Body {
    Full::new(bytes)
        .map_err(|never: Infallible| match never {})
        .boxed_unsync()
}

async fn origin(bytes: Bytes, faults: bool) -> Origin {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let source = format!("http://upstream.invalid:{}", address.port());
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let peak_task = peak.clone();
    let requests = recorded.clone();
    let throttle = Arc::new(AtomicBool::new(false));
    let unavailable = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let resolve = Arc::new(AtomicUsize::new(0));
    let resolver_hits = resolve.clone();
    let old_requests = Arc::new(AtomicUsize::new(0));
    let task = tokio::spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                _ = connections.join_next(), if !connections.is_empty() => {},
                connection = listener.accept() => {
                    let (socket, _) = connection.unwrap();
                    let (bytes, active, peak, requests, throttle, unavailable, dropped, resolve, old_requests) = (bytes.clone(), active.clone(), peak_task.clone(), requests.clone(), throttle.clone(), unavailable.clone(), dropped.clone(), resolve.clone(), old_requests.clone());
                    connections.spawn(async move {
                        let service = service_fn(move |request: http::Request<Incoming>| {
                            let (bytes, active, peak, requests, throttle, unavailable, dropped, resolve, old_requests) = (bytes.clone(), active.clone(), peak.clone(), requests.clone(), throttle.clone(), unavailable.clone(), dropped.clone(), resolve.clone(), old_requests.clone());
                            async move {
                                let path = request.uri().path(); let mut response = http::Response::new(full(Bytes::new()));
                                if path.starts_with("/hop/") {
                                    let hop = path.trim_start_matches("/hop/").parse::<usize>().unwrap();
                                    *response.status_mut() = http::StatusCode::FOUND; response.headers_mut().insert("location", format!("/hop/{}", hop+1).parse().unwrap()); return Ok::<_, Infallible>(response);
                                }
                                if matches!(path, "/redirect" | "/resolve" | "/unsafe" | "/credentials" | "/loopback" | "/loopback-dns" | "/mapped-loopback" | "/metadata") {
                                    let target = match path { "/loopback" => "http://127.0.0.1:1/private?secret=token".to_owned(), "/loopback-dns" => "http://localhost:1/private?secret=token".to_owned(), "/mapped-loopback" => "http://[::ffff:127.9.8.7]:1/private".to_owned(), "/metadata" => "http://169.254.169.254/private".to_owned(), "/unsafe" => "file:///secret".to_owned(), "/credentials" => "http://user:secret@localhost/film".to_owned(), "/resolve" => if resolve.fetch_add(1, Ordering::SeqCst) == 0 { "/old".to_owned() } else { "/new".to_owned() }, _ => "/film".to_owned() };
                                    *response.status_mut() = http::StatusCode::FOUND; response.headers_mut().insert("location", target.parse().unwrap()); return Ok(response);
                                }
                                assert!(request.headers().get("cookie").is_none()); assert_eq!(request.headers()["cache-control"], "no-store");
                                let range = request.headers()["range"].to_str().unwrap().strip_prefix("bytes=").unwrap(); let (start,end) = range.split_once('-').unwrap(); let start = start.parse::<usize>().unwrap(); let end = end.parse::<usize>().unwrap().min(bytes.len()-1);
                                lock(&requests).push((start as u64, tokio::time::Instant::now()));
                                if faults && start > 0 { assert_eq!(request.headers()["if-range"], "\"fixture-v1\""); assert_eq!(request.headers()["accept-encoding"], "identity"); }
                                if faults && path == "/old" && old_requests.fetch_add(1, Ordering::SeqCst) >= 10 { *response.status_mut() = http::StatusCode::FORBIDDEN; return Ok(response); }
                                if faults && start == CHUNK_BYTES as usize && !throttle.swap(true, Ordering::SeqCst) { *response.status_mut() = http::StatusCode::TOO_MANY_REQUESTS; response.headers_mut().insert("retry-after", "1".parse().unwrap()); return Ok(response); }
                                if faults && start == 2*CHUNK_BYTES as usize && !unavailable.swap(true, Ordering::SeqCst) { *response.status_mut() = http::StatusCode::SERVICE_UNAVAILABLE; return Ok(response); }
                                let drop_body = faults && start == 3*CHUNK_BYTES as usize && !dropped.swap(true, Ordering::SeqCst);
                                let count = active.fetch_add(1, Ordering::SeqCst)+1; peak.fetch_max(count, Ordering::SeqCst); let guard = Active(active);
                                *response.status_mut() = http::StatusCode::PARTIAL_CONTENT; response.headers_mut().insert("etag", "\"fixture-v1\"".parse().unwrap()); response.headers_mut().insert("content-range", format!("bytes {start}-{end}/{}", bytes.len()).parse().unwrap()); response.headers_mut().insert("content-length", (end-start+1).to_string().parse().unwrap());
                                let stream = futures::stream::unfold((bytes.slice(start..=end), 0_usize, Some(guard)), move |(bytes, offset, guard)| async move {
                                    if offset >= bytes.len() { return None; }
                                    if drop_body && offset > 0 { tokio::time::sleep(Duration::from_millis(50)).await; return Some((Err(MediaError::Transport), (bytes, usize::MAX, guard))); }
                                    if faults && start == 4*CHUNK_BYTES as usize && offset == 0 { tokio::time::sleep(Duration::from_millis(20)).await; }
                                    let length = if drop_body { (1024*1024).min(bytes.len()-offset) } else { 65536.min(bytes.len()-offset) };
                                    Some((Ok(Frame::data(bytes.slice(offset..offset+length))), (bytes, offset+length, guard)))
                                });
                                *response.body_mut() = StreamBody::new(stream).boxed_unsync(); Ok(response)
                            }
                        });
                        let _ = http1::Builder::new().serve_connection(TokioIo::new(socket), service).await;
                    });
                }
            }
        }
    });
    Origin {
        source,
        address,
        task,
        peak,
        requests: recorded,
        resolver_hits,
    }
}

#[tokio::test(start_paused = true)]
async fn manual_redirects_revalidate_each_hop_limit_ten_and_send_no_cookies() {
    let origin = origin(Bytes::from_static(b"fixture"), false).await;
    let fetch = HttpMediaFetch::for_test(origin.address).unwrap();
    let mut headers = http::HeaderMap::new();
    headers.insert("range", "bytes=0-0".parse().unwrap());
    headers.insert("cookie", "secret=session".parse().unwrap());
    let request = FetchRequest {
        method: http::Method::GET,
        headers,
    };
    let result = drive(fetch.fetch(
        MediaSource::new(&format!("{}/redirect", origin.source)).unwrap(),
        request.clone(),
    ))
    .await
    .unwrap();
    assert_eq!(result.status, 206);
    assert!(result.final_source.as_str().ends_with("/film"));
    for (path, error) in [
        ("/hop/0", MediaError::RedirectLimit),
        ("/unsafe", MediaError::InvalidDestination),
        ("/credentials", MediaError::InvalidDestination),
        ("/loopback", MediaError::InvalidDestination),
        ("/loopback-dns", MediaError::InvalidDestination),
        ("/mapped-loopback", MediaError::InvalidDestination),
        ("/metadata", MediaError::InvalidDestination),
    ] {
        let result = drive(fetch.fetch(
            MediaSource::new(&format!("{}{path}", origin.source)).unwrap(),
            request.clone(),
        ))
        .await;
        assert!(matches!(result, Err(ref found) if *found == error));
    }
}

#[tokio::test(start_paused = true)]
async fn forty_mib_faulted_upstream_sequential_reads_three_seeks_and_budgets() {
    let bytes = Bytes::from(
        (0..40 * 1_048_576_u64)
            .map(|index| {
                let mut value = index.wrapping_mul(0x9e3779b97f4a7c15);
                value ^= value >> 27;
                value = value.wrapping_mul(0x94d049bb133111eb);
                (value >> 31) as u8
            })
            .collect::<Vec<_>>(),
    );
    let origin = origin(bytes.clone(), true).await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = ProxyOptions::default();
    options.cache.directory = dir.path().to_owned();
    options.cache.max_memory_bytes = 24 * 1_048_576;
    options.cache.max_disk_bytes = 16 * 1_048_576;
    options.cache.reserve_free_bytes = 0;
    options.adaptive = false;
    let session = Arc::new(Mutex::new(None));
    let fetch = Arc::new(CheckedFetch {
        transport: HttpMediaFetch::for_test(origin.address).unwrap(),
        session: session.clone(),
    });
    let mut proxy = drive(MediaProxy::with_fetch(options, fetch, None))
        .await
        .unwrap();
    let handle = drive(
        proxy.create_session(MediaSource::new(&format!("{}/resolve", origin.source)).unwrap()),
    )
    .await
    .unwrap();
    *lock(&session) = Some(Arc::downgrade(&handle.session));
    handle.set_read_ahead(sample(300.0, false));
    let mut response = drive(reqwest::get(&handle.url)).await.unwrap();
    let mut position = 0;
    while let Some(part) = drive(response.chunk()).await.unwrap() {
        assert_eq!(part, bytes.slice(position..position + part.len()));
        position += part.len();
        let stats = proxy.cache_stats();
        assert!(stats.memory_bytes + stats.reserved_bytes <= 24 * 1_048_576);
        assert!(stats.disk_bytes <= 16 * 1_048_576);
    }
    assert_eq!(position, bytes.len());
    assert_eq!(origin.peak.load(Ordering::SeqCst), 1);
    handle.set_read_ahead(sample(2.0, true));
    let client = reqwest::Client::new();
    for (start, end) in [
        (31 * 1_048_576, 33 * 1_048_576 + 123),
        (4194000, 6300000),
        (39 * 1_048_576, 40 * 1_048_576 - 1),
    ] {
        let response = drive(
            client
                .get(&handle.url)
                .header("range", format!("bytes={start}-{end}"))
                .send(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 206);
        assert_eq!(
            drive(response.bytes()).await.unwrap(),
            bytes.slice(start..=end)
        );
        let stats = proxy.cache_stats();
        assert!(stats.memory_bytes + stats.reserved_bytes <= 24 * 1_048_576);
        assert!(stats.disk_bytes <= 16 * 1_048_576);
    }
    assert!(origin.peak.load(Ordering::SeqCst) <= handle.stats().parallel);
    assert!(handle.stats().parallel <= 6);
    assert_eq!(origin.resolver_hits.load(Ordering::SeqCst), 2);
    {
        let requests = lock(&origin.requests);
        let throttled: Vec<_> = requests
            .iter()
            .filter(|(start, _)| *start == CHUNK_BYTES)
            .map(|(_, time)| *time)
            .collect();
        assert!(throttled.len() >= 2);
        assert!(throttled[1].duration_since(throttled[0]) >= Duration::from_secs(1));
        assert!(
            requests
                .iter()
                .any(|(start, _)| *start > 3 * CHUNK_BYTES && *start < 4 * CHUNK_BYTES)
        );
    }
    drive(proxy.close()).await;
}
