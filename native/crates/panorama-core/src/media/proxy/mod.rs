//! Loopback proxy and lifecycle ported from `desktop/main/media-proxy.ts` and `loopback-port.ts`.

mod download;
mod events;
mod probe;
mod scheduler;
mod server;
mod session;

use super::{
    MediaError,
    cache::{CacheStats, MediaCache, MediaCacheOptions},
    fetch::{HttpMediaFetch, MediaFetch, MediaResolver, MediaSource},
    lock,
    policy::BufferingSample,
    token,
};
pub use events::{EventSink, MediaProxyEvent, ProbeResult};
use session::Session;
use std::{
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};

/// Range size used by the Electron proxy: 2 MiB.
pub const CHUNK_BYTES: u64 = 2 * 1_048_576;

/// Proxy cache, policy and timeout configuration.
#[derive(Clone, Debug)]
pub struct ProxyOptions {
    /// Cache budgets and parent directory.
    pub cache: MediaCacheOptions,
    /// Per-byte inactivity timeout (default eight seconds).
    pub stall_timeout: Duration,
    /// Enables the measured BufferingPolicy instead of fixed buffer thresholds.
    pub adaptive: bool,
    /// Deadline for draining accepted connections before aborting them.
    pub shutdown_timeout: Duration,
}
impl Default for ProxyOptions {
    fn default() -> Self {
        Self {
            cache: MediaCacheOptions {
                directory: std::env::temp_dir().join("panorama-media-cache"),
                max_memory_bytes: 96 * 1_048_576,
                max_disk_bytes: 2 * 1_073_741_824,
                reserve_free_bytes: 1_073_741_824,
            },
            stall_timeout: Duration::from_secs(8),
            adaptive: std::env::var("PANORAMA_ADAPTIVE_BUFFERING").is_ok_and(|value| value == "1"),
            shutdown_timeout: Duration::from_millis(250),
        }
    }
}

/// Safe player diagnostics without source URLs.
#[derive(Clone, Debug)]
pub struct MediaProxyStats {
    /// Recent throughput; None when no transfers are active.
    pub download_mbps: Option<f64>,
    /// Probed representation size; None for passthrough.
    pub size_bytes: Option<u64>,
    /// Source transport recently refused a probe.
    pub unreachable: bool,
    /// Integrity validation failed.
    pub representation_changed: bool,
    /// Reader and upstream transfer are both active.
    pub transfer_demanded: bool,
    /// Current connection cap.
    pub parallel: usize,
    /// Forward buffer target.
    pub target_ahead_seconds: u32,
    /// MPV cache-pause-wait threshold.
    pub resume_buffer_seconds: u32,
    /// Shared cache budget accounting.
    pub cache: CacheStats,
    /// Terminal sanitized session failure.
    pub error: Option<MediaError>,
}

/// Warm-up status for one source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaWarmResult {
    /// True if probing succeeded.
    pub ready: bool,
    /// Sanitized probe failure.
    pub error: Option<MediaError>,
    /// True during the two-minute unreachable hold.
    pub unreachable: bool,
}

/// Playback URL and lifetime controls; Debug never includes URL or token.
pub struct SessionHandle {
    /// Loopback URL passed to MPV.
    pub url: String,
    session: Arc<Session>,
    registry: std::sync::Weak<Shared>,
    token: String,
}
impl fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionHandle([REDACTED])")
    }
}
impl SessionHandle {
    /// Feeds player measurements and updates upstream demand.
    pub fn set_read_ahead(&self, sample: BufferingSample) {
        self.session.set_read_ahead(sample);
    }
    /// Returns diagnostics for this session.
    pub fn stats(&self) -> MediaProxyStats {
        self.session.stats()
    }
    /// Aborts transfers and explicitly removes this session's cache entries.
    pub async fn close(&self) {
        if let Some(registry) = self.registry.upgrade() {
            lock(&registry.sessions).remove(&self.token);
        }
        self.session.close().await;
    }
}

pub(super) struct Shared {
    sessions: Mutex<std::collections::HashMap<String, Arc<Session>>>,
    host: String,
    stop: watch::Sender<bool>,
}
struct Warmed {
    source: MediaSource,
    session: Arc<Session>,
    expires: tokio::time::Instant,
}

/// Ephemeral IPv4 loopback listener, owning every connection and transfer task.
pub struct MediaProxy {
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
    cache: MediaCache,
    fetch: Arc<dyn MediaFetch>,
    resolver: Option<Arc<dyn MediaResolver>>,
    options: ProxyOptions,
    warmed: Option<Warmed>,
    warm_timer: Option<JoinHandle<()>>,
    observer: Arc<Mutex<Option<EventSink>>>,
}

impl MediaProxy {
    /// Starts the production rustls transport on 127.0.0.1 port zero.
    pub async fn start(
        options: ProxyOptions,
        resolver: Option<Arc<dyn MediaResolver>>,
    ) -> Result<Self, MediaError> {
        Self::with_fetch(options, Arc::new(HttpMediaFetch::new()?), resolver).await
    }
    /// Starts with an injected upstream for isolated tests or host integration.
    pub async fn with_fetch(
        options: ProxyOptions,
        fetch: Arc<dyn MediaFetch>,
        resolver: Option<Arc<dyn MediaResolver>>,
    ) -> Result<Self, MediaError> {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| MediaError::Transport)?;
        let port = listener
            .local_addr()
            .map_err(|_| MediaError::Transport)?
            .port();
        let cache = MediaCache::open(options.cache.clone()).await?;
        let (stop, _) = watch::channel(false);
        let shared = Arc::new(Shared {
            sessions: Default::default(),
            host: format!("127.0.0.1:{port}"),
            stop,
        });
        let accept = tokio::spawn(server::accept(
            listener,
            shared.clone(),
            options.shutdown_timeout,
        ));
        Ok(Self {
            shared,
            accept: Some(accept),
            cache,
            fetch,
            resolver,
            options,
            warmed: None,
            warm_timer: None,
            observer: Arc::new(Mutex::new(None)),
        })
    }
    fn session(&self, source: MediaSource) -> Result<Arc<Session>, MediaError> {
        Session::new(
            source,
            self.fetch.clone(),
            self.resolver.clone(),
            self.cache.clone(),
            &self.options,
            self.observer.clone(),
        )
    }
    /// Replaces prior playback, adopting a matching unexpired warm-up.
    pub async fn create_session(
        &mut self,
        source: MediaSource,
    ) -> Result<SessionHandle, MediaError> {
        if *self.shared.stop.borrow() {
            return Err(MediaError::Closed);
        }
        let previous: Vec<_> = lock(&self.shared.sessions)
            .drain()
            .map(|(_, session)| session)
            .collect();
        for session in previous {
            session.close().await;
        }
        let adopted = self.warmed.as_ref().is_some_and(|warm| {
            warm.source == source && tokio::time::Instant::now() < warm.expires
        });
        let session = if adopted {
            if let Some(timer) = self.warm_timer.take() {
                timer.abort();
                let _ = timer.await;
            }
            self.warmed
                .take()
                .map(|warm| warm.session)
                .ok_or(MediaError::Closed)?
        } else {
            self.session(source)?
        };
        let token = token()?;
        lock(&self.shared.sessions).insert(token.clone(), session.clone());
        Ok(SessionHandle {
            url: format!("http://{}/media/{token}", self.shared.host),
            session,
            registry: Arc::downgrade(&self.shared),
            token,
        })
    }
    /// Warms the first three chunks and the tail; keeps at most one source for ten minutes.
    pub async fn warm(&mut self, source: MediaSource) -> Result<MediaWarmResult, MediaError> {
        if *self.shared.stop.borrow() {
            return Err(MediaError::Closed);
        }
        if !self
            .warmed
            .as_ref()
            .is_some_and(|warm| warm.source == source && tokio::time::Instant::now() < warm.expires)
        {
            self.discard_warm().await;
            let session = self.session(source.clone())?;
            let expires = tokio::time::Instant::now() + Duration::from_secs(600);
            let weak = Arc::downgrade(&session);
            self.warm_timer = Some(tokio::spawn(async move {
                tokio::time::sleep_until(expires).await;
                if let Some(session) = weak.upgrade() {
                    session.close().await;
                }
            }));
            self.warmed = Some(Warmed {
                source,
                session,
                expires,
            });
        }
        match &self.warmed {
            Some(warm) => {
                let result = warm.session.warm().await;
                warm.session.emit(MediaProxyEvent::Warm {
                    ready: result.ready,
                    error: result.error.clone(),
                });
                Ok(result)
            }
            None => Err(MediaError::Closed),
        }
    }
    async fn discard_warm(&mut self) {
        if let Some(timer) = self.warm_timer.take() {
            timer.abort();
            let _ = timer.await;
        }
        if let Some(warm) = self.warmed.take() {
            warm.session.close().await;
        }
    }
    /// Stops accepting, cancels upstream work, bounds connection drain and deletes owned cache.
    pub async fn close(&mut self) {
        self.shared.stop.send_replace(true);
        self.discard_warm().await;
        let sessions: Vec<_> = lock(&self.shared.sessions)
            .drain()
            .map(|(_, session)| session)
            .collect();
        for session in sessions {
            session.close().await;
        }
        if let Some(accept) = self.accept.take() {
            let _ = accept.await;
        }
        self.cache.close().await;
    }
    /// Current shared cache budget accounting.
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }
    /// Installs structured diagnostics for existing and future sessions.
    pub fn set_event_sink(&self, sink: Option<EventSink>) {
        *lock(&self.observer) = sink;
    }
    /// Cache root supplied by the caller.
    pub fn cache_directory(&self) -> &std::path::Path {
        &self.options.cache.directory
    }
}
impl Drop for MediaProxy {
    fn drop(&mut self) {
        self.shared.stop.send_replace(true);
        if let Some(accept) = self.accept.take() {
            accept.abort();
        }
        if let Some(timer) = self.warm_timer.take() {
            timer.abort();
        }
        if let Some(warm) = self.warmed.take() {
            warm.session.stop();
        }
        for (_, session) in lock(&self.shared.sessions).drain() {
            session.stop();
        }
        self.cache.close_now();
    }
}

#[cfg(test)]
mod tests;
