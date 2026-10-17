//! Session state and probing ported from `desktop/main/media-proxy.ts`.

use super::{CHUNK_BYTES, MediaProxyStats, ProxyOptions};
use super::{EventSink, MediaProxyEvent};
use crate::media::{
    MediaError,
    cache::MediaCache,
    fetch::{FetchRequest, FetchResponse, MediaFetch, MediaResolver, MediaSource},
    lock,
    policy::{BufferingPolicy, BufferingSample},
    retry::retry_after_deadline,
    token,
};
use bytes::{Bytes, BytesMut};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{Mutex as AsyncMutex, Notify, watch},
    task::JoinHandle,
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Mode {
    Ranged,
    Passthrough,
}
pub(super) struct ChunkState {
    pub data: BytesMut,
    pub complete: Option<Bytes>,
    pub error: Option<MediaError>,
}
pub(super) struct Chunk {
    pub data: Mutex<ChunkState>,
    pub notify: Notify,
    pub stop: watch::Sender<bool>,
    pub key: String,
    pub cache: MediaCache,
    pub lease: Mutex<Option<u64>>,
    pub running: std::sync::atomic::AtomicBool,
}
impl Drop for Chunk {
    fn drop(&mut self) {
        if let Some(lease) = lock(&self.lease).take() {
            self.cache.release(&self.key, lease);
        }
    }
}
pub(super) struct Reader {
    pub position: u64,
    pub foreground: bool,
}
pub(super) struct State {
    pub mode: Option<Mode>,
    pub size: Option<u64>,
    pub source: MediaSource,
    pub validator: Option<String>,
    pub generation: u64,
    pub content_type: String,
    pub error: Option<MediaError>,
    pub representation_changed: bool,
    pub unreachable_at: Option<Instant>,
    pub chunks: HashMap<u64, Arc<Chunk>>,
    pub chunk_order: VecDeque<u64>,
    pub readers: HashMap<u64, Reader>,
    pub next_reader: u64,
    pub ever_served: bool,
    pub demand: usize,
    pub stall_cap: usize,
    pub target: u32,
    pub window: u64,
    pub prefetch: bool,
    pub paused_until: Instant,
    pub cooldown: Duration,
    pub last_stall: Instant,
    pub active: usize,
    pub samples: VecDeque<(Instant, usize)>,
    pub policy: BufferingPolicy,
    pub resolves: VecDeque<Instant>,
    pub idle_generation: u64,
    pub idle_armed: bool,
    pub schedule_armed: bool,
}
impl State {
    pub fn parallel(&self) -> usize {
        self.demand.min(self.stall_cap)
    }
    pub fn rate(&mut self) -> Option<f64> {
        let now = Instant::now();
        while self
            .samples
            .front()
            .is_some_and(|(time, _)| now.duration_since(*time) > Duration::from_secs(5))
        {
            self.samples.pop_front();
        }
        (self.active > 0).then(|| {
            self.samples
                .iter()
                .map(|(_, bytes)| *bytes as f64)
                .sum::<f64>()
                * 8.0
                / 5.0
                / 1_000_000.0
        })
    }
}

pub(super) struct Session {
    pub original: MediaSource,
    pub fetch: Arc<dyn MediaFetch>,
    pub resolver: Option<Arc<dyn MediaResolver>>,
    pub cache: MediaCache,
    pub id: String,
    pub state: Mutex<State>,
    pub stop_signal: watch::Sender<bool>,
    pub tasks: Mutex<Vec<JoinHandle<()>>>,
    pub prepare_lock: AsyncMutex<()>,
    pub resolve_lock: AsyncMutex<()>,
    pub stall_timeout: Duration,
    pub base_cooldown: Duration,
    pub max_cooldown: Duration,
    pub adaptive: bool,
    pub observer: Arc<Mutex<Option<EventSink>>>,
    pub created_at: Instant,
    pub unix_epoch_ms: u64,
}
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session([REDACTED])")
    }
}

pub(super) async fn cancelled(mut signal: watch::Receiver<bool>) {
    if *signal.borrow() {
        return;
    }
    while signal.changed().await.is_ok() {
        if *signal.borrow() {
            return;
        }
    }
}

impl Session {
    pub fn new(
        source: MediaSource,
        fetch: Arc<dyn MediaFetch>,
        resolver: Option<Arc<dyn MediaResolver>>,
        cache: MediaCache,
        options: &ProxyOptions,
        observer: Arc<Mutex<Option<EventSink>>>,
    ) -> Result<Arc<Self>, MediaError> {
        let now = Instant::now();
        let base_cooldown = options.stall_timeout.mul_f64(0.25);
        let (stop_signal, _) = watch::channel(false);
        Ok(Arc::new(Self {
            original: source.clone(),
            fetch,
            resolver,
            cache,
            id: format!("{}:", token()?),
            state: Mutex::new(State {
                mode: None,
                size: None,
                source,
                validator: None,
                generation: 0,
                content_type: "application/octet-stream".into(),
                error: None,
                representation_changed: false,
                unreachable_at: None,
                chunks: Default::default(),
                chunk_order: Default::default(),
                readers: Default::default(),
                next_reader: 0,
                ever_served: false,
                demand: if options.adaptive { 3 } else { 6 },
                stall_cap: 6,
                target: 60,
                window: 48,
                prefetch: true,
                paused_until: now,
                cooldown: base_cooldown,
                last_stall: now,
                active: 0,
                samples: Default::default(),
                policy: BufferingPolicy::new(),
                resolves: Default::default(),
                idle_generation: 0,
                idle_armed: false,
                schedule_armed: false,
            }),
            stop_signal,
            tasks: Default::default(),
            prepare_lock: AsyncMutex::new(()),
            resolve_lock: AsyncMutex::new(()),
            stall_timeout: options.stall_timeout,
            base_cooldown,
            max_cooldown: options.stall_timeout.mul_f64(3.75),
            adaptive: options.adaptive,
            observer,
            created_at: now,
            unix_epoch_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_millis().min(u64::MAX as u128) as u64)
                .unwrap_or(0),
        }))
    }
    pub fn key(&self, index: u64) -> String {
        format!("{}{index}", self.id)
    }
    pub fn emit(&self, event: MediaProxyEvent) {
        let observer = lock(&self.observer).clone();
        if let Some(observer) = observer {
            observer(event);
        }
    }
    pub fn spawn(&self, future: impl std::future::Future<Output = ()> + Send + 'static) {
        let mut tasks = lock(&self.tasks);
        tasks.retain(|task| !task.is_finished());
        if !*self.stop_signal.borrow() {
            tasks.push(tokio::spawn(future));
        }
    }
    pub fn stopped(&self) -> bool {
        *self.stop_signal.borrow()
    }
    pub fn stop(&self) {
        self.stop_signal.send_replace(true);
        for task in lock(&self.tasks).iter() {
            task.abort();
        }
        let mut state = lock(&self.state);
        for chunk in state.chunks.values() {
            chunk.stop.send_replace(true);
            chunk.notify.notify_waiters();
        }
        state.chunks.clear();
        state.chunk_order.clear();
    }
    pub async fn close(&self) {
        self.stop();
        let tasks = std::mem::take(&mut *lock(&self.tasks));
        for task in tasks {
            let _ = task.await;
        }
        self.cache.remove_session(&self.id).await;
    }
    pub fn fail(&self, error: MediaError) {
        {
            let mut state = lock(&self.state);
            state.representation_changed |= matches!(error, MediaError::Representation(_));
            state.error = Some(error);
        }
        self.stop_signal.send_replace(true);
        let state = lock(&self.state);
        for chunk in state.chunks.values() {
            chunk.stop.send_replace(true);
            chunk.notify.notify_waiters();
        }
    }
    pub fn unreachable(&self) -> bool {
        lock(&self.state)
            .unreachable_at
            .is_some_and(|time| time.elapsed() < Duration::from_secs(120))
    }
    pub fn stats(&self) -> MediaProxyStats {
        let mut state = lock(&self.state);
        MediaProxyStats {
            download_mbps: state.rate(),
            size_bytes: state.size,
            unreachable: state
                .unreachable_at
                .is_some_and(|time| time.elapsed() < Duration::from_secs(120)),
            representation_changed: state.representation_changed,
            transfer_demanded: state.active > 0 && !state.readers.is_empty(),
            parallel: state.parallel(),
            target_ahead_seconds: state.target,
            resume_buffer_seconds: 5,
            cache: self.cache.stats(),
            error: state.error.clone(),
        }
    }
    pub fn set_read_ahead(self: &Arc<Self>, mut sample: BufferingSample) {
        let mut state = lock(&self.state);
        state.demand =
            if sample.buffering || sample.buffered_seconds.is_none_or(|seconds| seconds < 20.0) {
                6
            } else if sample
                .buffered_seconds
                .is_some_and(|seconds| seconds < 60.0)
            {
                3
            } else {
                1
            };
        if state.stall_cap < 6 && state.last_stall.elapsed() > Duration::from_secs(60) {
            state.stall_cap += 1;
            state.last_stall = Instant::now();
        }
        if self.adaptive {
            sample.download_mbps = state.rate();
            sample.transfer_demanded = state.active > 0 && !state.readers.is_empty();
            sample.throttled = Instant::now() < state.paused_until;
            let decision = state.policy.update(&sample);
            state.demand = decision.parallel;
            state.target = decision.target_ahead_seconds;
            state.window = sample
                .source_mbps
                .filter(|rate| *rate > 0.0)
                .map(|rate| {
                    (rate * 125000.0 * f64::from(state.target) / CHUNK_BYTES as f64)
                        .ceil()
                        .clamp(3.0, 48.0) as u64
                })
                .unwrap_or(30);
        }
        drop(state);
        self.schedule();
    }
    pub async fn cooldown(&self) -> Result<(), MediaError> {
        loop {
            let until = lock(&self.state).paused_until;
            if Instant::now() >= until {
                return if self.stopped() {
                    Err(MediaError::Closed)
                } else {
                    Ok(())
                };
            }
            tokio::select! { _ = cancelled(self.stop_signal.subscribe()) => return Err(MediaError::Closed), _ = tokio::time::sleep_until(until) => {} }
        }
    }
    pub fn throttle(&self, retry: Option<&str>) {
        let unix_ms = self
            .unix_epoch_ms
            .saturating_add(self.created_at.elapsed().as_millis().min(u64::MAX as u128) as u64);
        let server_wait = retry_after_deadline(retry, unix_ms)
            .map(|deadline| Duration::from_millis(deadline.saturating_sub(unix_ms)))
            .unwrap_or_default();
        let mut state = lock(&self.state);
        state.stall_cap = state.stall_cap.saturating_sub(1).max(1);
        state.last_stall = Instant::now();
        let wait = server_wait.max(state.cooldown);
        if let Some(until) = Instant::now().checked_add(wait) {
            state.paused_until = state.paused_until.max(until);
        }
        state.cooldown = (state.cooldown * 2).min(self.max_cooldown);
    }
    pub async fn request(
        &self,
        source: MediaSource,
        start: u64,
        end: u64,
        validator: Option<String>,
        timeout: Duration,
    ) -> Result<FetchResponse, MediaError> {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "range",
            http::HeaderValue::from_str(&format!("bytes={start}-{end}"))
                .map_err(|_| MediaError::Transport)?,
        );
        headers.insert(
            "accept-encoding",
            http::HeaderValue::from_static("identity"),
        );
        if let Some(validator) = validator {
            headers.insert(
                "if-range",
                http::HeaderValue::from_str(&validator).map_err(|_| MediaError::Transport)?,
            );
        }
        tokio::select! {
            _ = cancelled(self.stop_signal.subscribe()) => Err(MediaError::Closed),
            result = tokio::time::timeout(timeout, self.fetch.fetch(source, FetchRequest { method: http::Method::GET, headers })) => result.map_err(|_| MediaError::Timeout)?,
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}
