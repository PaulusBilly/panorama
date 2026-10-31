//! Injectable deterministic upstream for `tests/unit/media-proxy.test.ts` ports.
use super::*;
use crate::media::fetch::{FetchRequest, FetchResponse, MediaFetch};
use futures::{StreamExt, future::BoxFuture};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
pub struct Faults {
    pub ranged: bool,
    pub expire: bool,
    pub slow: bool,
    pub drop_range: bool,
    pub stalls: usize,
    pub probe_throttle: bool,
    pub pending_tail: bool,
    pub pending_all: bool,
    pub failed: bool,
    pub changed_size: bool,
    pub delay: bool,
}
pub struct Data {
    pub bytes: Bytes,
    pub faults: Mutex<Faults>,
    pub ranges: Mutex<Vec<String>>,
    pub opening: AtomicUsize,
    pub resolver_hits: AtomicUsize,
    pub held: AtomicUsize,
    pub dropped: AtomicUsize,
    pub open: AtomicUsize,
    pub peak: AtomicUsize,
}
#[derive(Clone)]
pub struct Fixture(pub Arc<Data>);
impl Fixture {
    pub fn new() -> Self {
        Self(Arc::new(Data {
            bytes: Bytes::from(
                (0..9 * 1_048_576 + 123)
                    .map(|index| (index % 251) as u8)
                    .collect::<Vec<_>>(),
            ),
            faults: Mutex::new(Faults {
                ranged: true,
                ..Default::default()
            }),
            ranges: Default::default(),
            opening: AtomicUsize::new(0),
            resolver_hits: AtomicUsize::new(0),
            held: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            open: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }))
    }
    pub fn source() -> MediaSource {
        MediaSource::new("http://fixture.invalid/resolve?secret=do-not-log").unwrap()
    }
    pub fn ranges(&self) -> Vec<String> {
        lock(&self.0.ranges).clone()
    }
}
struct Open(Arc<Data>);
impl Drop for Open {
    fn drop(&mut self) {
        self.0.open.fetch_sub(1, Ordering::SeqCst);
    }
}
impl MediaFetch for Fixture {
    fn fetch(
        &self,
        source: MediaSource,
        request: FetchRequest,
    ) -> BoxFuture<'static, Result<FetchResponse, MediaError>> {
        let fixture = self.clone();
        Box::pin(async move {
            let raw = crate::media::range::header(&request.headers, "range")
                .unwrap_or("")
                .to_owned();
            lock(&fixture.0.ranges).push(raw.clone());
            let mut final_source = source.clone();
            let (start, end) = raw
                .strip_prefix("bytes=")
                .and_then(|value| value.split_once('-'))
                .map(|(start, end)| {
                    (
                        start.parse::<usize>().unwrap(),
                        end.parse::<usize>().unwrap(),
                    )
                })
                .unwrap_or((0, fixture.0.bytes.len() - 1));
            let (
                ranged,
                expire,
                slow,
                drop_range,
                stalls,
                probe_throttle,
                pending_tail,
                pending_all,
                failed,
                changed_size,
                delay,
            ) = {
                let fault = lock(&fixture.0.faults);
                (
                    fault.ranged,
                    fault.expire,
                    fault.slow,
                    fault.drop_range,
                    fault.stalls,
                    fault.probe_throttle,
                    fault.pending_tail,
                    fault.pending_all,
                    fault.failed,
                    fault.changed_size,
                    fault.delay,
                )
            };
            if failed {
                return Err(MediaError::Transport);
            }
            let opening = if start == 0 && end > 0 {
                fixture.0.opening.fetch_add(1, Ordering::SeqCst)
            } else {
                0
            };
            let mut status = if ranged { 206 } else { 200 };
            if source.as_str().contains("/resolve") {
                let hit = fixture.0.resolver_hits.fetch_add(1, Ordering::SeqCst);
                final_source = MediaSource::new(if hit == 0 {
                    "http://fixture.invalid/old"
                } else {
                    "http://fixture.invalid/new"
                })
                .unwrap();
            }
            if expire && source.as_str().contains("/old") && start > 0 {
                status = 403;
            }
            if probe_throttle && start == 0 && opening == 0 && end > 0 {
                status = 503;
            }
            let count = fixture.0.open.fetch_add(1, Ordering::SeqCst) + 1;
            fixture.0.peak.fetch_max(count, Ordering::SeqCst);
            let guard = Open(fixture.0.clone());
            if stalls > 0
                && start > 0
                && fixture
                    .0
                    .held
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |held| {
                        (held < stalls).then_some(held + 1)
                    })
                    .is_ok()
            {
                futures::future::pending::<()>().await;
            }
            if delay {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let total = fixture.0.bytes.len()
                + usize::from(changed_size && source.as_str().contains("/new"));
            let end = end.min(fixture.0.bytes.len() - 1);
            let mut headers = http::HeaderMap::new();
            headers.insert("etag", "\"fixture-v1\"".parse().unwrap());
            headers.insert("content-type", "video/x-matroska".parse().unwrap());
            let bytes = if ranged {
                fixture.0.bytes.slice(start..=end)
            } else {
                fixture.0.bytes.clone()
            };
            headers.insert("content-length", bytes.len().to_string().parse().unwrap());
            if ranged {
                headers.insert(
                    "content-range",
                    format!("bytes {start}-{end}/{total}").parse().unwrap(),
                );
            }
            let drop_body = drop_range
                && start == CHUNK_BYTES as usize
                && fixture.0.dropped.fetch_add(1, Ordering::SeqCst) == 0;
            let pending = pending_all || (pending_tail && start == 8 * 1_048_576);
            let body = if status != 206 && status != 200 {
                Box::pin(futures::stream::empty()) as crate::media::fetch::MediaBody
            } else {
                Box::pin(futures::stream::unfold(
                    (bytes, 0_usize, Some(guard)),
                    move |(bytes, position, guard)| async move {
                        if position >= bytes.len() {
                            return None;
                        }
                        if pending && position > 0 {
                            futures::future::pending::<()>().await;
                        }
                        if drop_body && position > 0 {
                            return Some((Err(MediaError::Transport), (bytes, usize::MAX, guard)));
                        }
                        if slow && start == 0 && position == 65536 {
                            tokio::time::sleep(Duration::from_millis(400)).await;
                        }
                        let length = if drop_body {
                            1_000_000.min(bytes.len() - position)
                        } else if pending {
                            1
                        } else {
                            65536.min(bytes.len() - position)
                        };
                        let part = bytes.slice(position..position + length);
                        Some((Ok(part), (bytes, position + length, guard)))
                    },
                )) as crate::media::fetch::MediaBody
            };
            Ok(FetchResponse {
                status,
                headers,
                body: body.boxed(),
                final_source,
            })
        })
    }
}

pub async fn setup(
    fixture: &Fixture,
    adaptive: bool,
) -> (tempfile::TempDir, MediaProxy, SessionHandle) {
    let dir = tempfile::tempdir().unwrap();
    let mut options = ProxyOptions::default();
    options.cache.directory = dir.path().to_owned();
    options.cache.reserve_free_bytes = 0;
    options.adaptive = adaptive;
    let mut proxy = MediaProxy::with_fetch(options, Arc::new(fixture.clone()), None)
        .await
        .unwrap();
    let handle = proxy.create_session(Fixture::source()).await.unwrap();
    (dir, proxy, handle)
}
pub async fn read(handle: &SessionHandle, start: u64, end: u64) -> Result<Bytes, MediaError> {
    handle.session.prepare().await?;
    let mut reader = handle.session.add_reader(start)?;
    let mut bytes = Vec::new();
    while let Some(part) = reader.read(end).await? {
        bytes.extend_from_slice(&part);
    }
    Ok(Bytes::from(bytes))
}
pub async fn drive<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::pin!(future);
    let mut ticks = 0;
    loop {
        tokio::select! { biased; result = &mut future => return result, _ = tokio::task::yield_now() => {} }
        ticks += 1;
        if ticks % 1000 == 0 {
            tokio::time::advance(Duration::from_millis(50)).await;
        }
        assert!(ticks < 5_000_000, "operation failed to make progress");
    }
}
pub async fn until(condition: impl Fn() -> bool) {
    for _ in 0..100_000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("condition never became true");
}
pub fn sample(ahead: f64, buffering: bool) -> BufferingSample {
    BufferingSample {
        now_ms: 0,
        buffered_seconds: Some(ahead),
        source_mbps: None,
        download_mbps: None,
        transfer_demanded: false,
        buffering,
        paused: false,
        playback_speed: 1.0,
        throttled: false,
    }
}
