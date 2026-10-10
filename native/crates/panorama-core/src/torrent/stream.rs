use super::cache::Cache;
use super::{TorrentError, TorrentState, TorrentStats, lock, runtime::Host};
use librqbit::{ManagedTorrent, Session};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub(super) struct Activity {
    pub opens: usize,
    pub last_used: Instant,
    pub last_progress: Instant,
    last_retry: Instant,
    previous_fetched: u64,
    fetched_offset: u64,
    snapshot: TorrentStats,
}

pub(super) struct Entry {
    pub torrent: Arc<ManagedTorrent>,
    pub session: Weak<Session>,
    pub cache: Arc<Cache>,
    pub readers: AtomicUsize,
    pub control: tokio::sync::Mutex<()>,
    pub activity: Mutex<Activity>,
    pub selected: Mutex<HashSet<usize>>,
    read_files: Mutex<HashMap<usize, usize>>,
    refresh: AtomicBool,
}

impl Entry {
    pub fn new(
        torrent: Arc<ManagedTorrent>,
        session: Arc<Session>,
        cache: Arc<Cache>,
        index: usize,
    ) -> Self {
        Self {
            torrent,
            session: Arc::downgrade(&session),
            cache,
            readers: AtomicUsize::new(0),
            control: tokio::sync::Mutex::new(()),
            selected: Mutex::new(HashSet::from([index])),
            read_files: Mutex::new(HashMap::new()),
            refresh: AtomicBool::new(false),
            activity: Mutex::new(Activity {
                opens: 0,
                last_used: Instant::now(),
                last_progress: Instant::now(),
                last_retry: Instant::now(),
                previous_fetched: 0,
                fetched_offset: 0,
                snapshot: TorrentStats {
                    peers_live: 0,
                    peers_connecting: 0,
                    downloaded_bytes: 0,
                    verified_bytes: 0,
                    download_bps: 0,
                    upload_bps: 0,
                    state: TorrentState::Connecting,
                    error: None,
                },
            }),
        }
    }

    pub fn snapshot(&self) -> TorrentStats {
        let mut activity = lock(&self.activity);
        let raw = self.torrent.stats();
        let mut stats = activity.snapshot.clone();
        let selected = lock(&self.selected);
        let verified: u64 = selected
            .iter()
            .filter_map(|index| raw.file_progress.get(*index))
            .sum();
        stats.verified_bytes = stats.verified_bytes.max(verified);
        drop(selected);
        if let Some(live) = raw.live {
            let fetched = live.snapshot.fetched_bytes;
            if fetched < activity.previous_fetched {
                activity.fetched_offset += activity.previous_fetched;
            }
            activity.previous_fetched = fetched;
            stats.downloaded_bytes = activity.fetched_offset + fetched;
            stats.peers_live = live.snapshot.peer_stats.live;
            stats.peers_connecting = live.snapshot.peer_stats.connecting;
            stats.download_bps = live.download_speed.as_bytes();
            stats.upload_bps = live.upload_speed.as_bytes();
        } else {
            stats.peers_live = 0;
            stats.peers_connecting = 0;
            stats.download_bps = 0;
            stats.upload_bps = 0;
        }
        if stats.downloaded_bytes > activity.snapshot.downloaded_bytes
            || self.torrent.is_paused()
            || self.readers.load(Ordering::Acquire) == 0
        {
            activity.last_progress = Instant::now();
        }
        stats.error = lock(&self.cache.error).or(stats.error);
        if matches!(raw.state, librqbit::TorrentStatsState::Error) && stats.error.is_none() {
            stats.error = Some(TorrentError::Engine);
        }
        stats.state = if stats.error.is_some() {
            TorrentState::Stalled
        } else if matches!(raw.state, librqbit::TorrentStatsState::Initializing { .. }) {
            TorrentState::Metadata
        } else if stats.verified_bytes > 0 {
            TorrentState::Streaming
        } else {
            TorrentState::Connecting
        };
        activity.snapshot = stats.clone();
        stats
    }

    pub fn fail(&self, error: TorrentError) {
        lock(&self.activity).snapshot.error.get_or_insert(error);
    }

    pub async fn monitor(&self, timeout: Duration) {
        let Ok(_guard) = self.control.try_lock() else {
            return;
        };
        let stats = self.snapshot();
        let unfinished = self.torrent.metadata.load_full().is_some_and(|metadata| {
            let raw = self.torrent.stats();
            lock(&self.read_files).keys().any(|index| {
                metadata.file_infos.get(*index).is_some_and(|file| {
                    raw.file_progress.get(*index).copied().unwrap_or(0) < file.len
                })
            })
        });
        {
            let mut activity = lock(&self.activity);
            if self.readers.load(Ordering::Acquire) > 0
                && stats.peers_live == 0
                && stats.peers_connecting == 0
                && activity.last_progress.elapsed() >= timeout
                && unfinished
            {
                activity.snapshot.error = Some(TorrentError::NoPeers);
            }
        }
        if let Some(metadata) = self.torrent.metadata.load_full() {
            let files: Vec<_> = lock(&self.read_files)
                .keys()
                .filter_map(|index| {
                    metadata
                        .file_infos
                        .get(*index)
                        .map(|file| (*index, file.len))
                })
                .collect();
            if self.cache.exhausted_for(&files) {
                lock(&self.cache.error).get_or_insert(TorrentError::Disk(super::DiskError::Full));
            }
        }
        {
            let stats = self.snapshot();
            let inactive = self.readers.load(Ordering::Acquire) == 0;
            let refresh = self.refresh.load(Ordering::Acquire);
            if let Some(session) = self.session.upgrade() {
                if (inactive || stats.error.is_some()) && !self.torrent.is_paused() {
                    let _ = session.pause(&self.torrent).await;
                    self.refresh.store(false, Ordering::Release);
                } else if refresh && (stats.peers_live > 0 || stats.peers_connecting > 0) {
                    self.refresh.store(false, Ordering::Release);
                } else if !inactive
                    && unfinished
                    && stats.error.is_none()
                    && stats.peers_live == 0
                    && stats.peers_connecting == 0
                    && (refresh
                        || lock(&self.activity).last_retry.elapsed() >= Duration::from_millis(500))
                {
                    self.refresh.store(false, Ordering::Release);
                    lock(&self.activity).last_retry = Instant::now();
                    if !self.torrent.is_paused() {
                        let _ = session.pause(&self.torrent).await;
                    }
                    let _ = session
                        .update_only_files(&self.torrent, &HashSet::new())
                        .await;
                    self.resume(&session, refresh).await.ok();
                }
            }
        }
    }

    async fn resume(
        &self,
        session: &Arc<Session>,
        reset_progress: bool,
    ) -> Result<(), TorrentError> {
        if self.torrent.is_paused() {
            {
                let mut activity = lock(&self.activity);
                activity.fetched_offset += activity.previous_fetched;
                activity.previous_fetched = 0;
                if reset_progress {
                    activity.last_progress = Instant::now();
                }
            }
            session
                .unpause(&self.torrent)
                .await
                .map_err(|_| TorrentError::Engine)?;
        }
        Ok(())
    }

    pub async fn retry(&self) -> Result<(), TorrentError> {
        let _guard = self.control.lock().await;
        let mut activity = lock(&self.activity);
        if activity.opens == 0
            && self.readers.load(Ordering::Acquire) == 0
            && activity.snapshot.error == Some(TorrentError::NoPeers)
        {
            activity.snapshot.error = None;
            activity.last_progress = Instant::now();
            activity.last_retry = Instant::now();
            self.refresh.store(false, Ordering::Release);
        }
        Ok(())
    }

    pub async fn reader(self: &Arc<Self>, index: usize) -> Result<Reader, TorrentError> {
        let _guard = self.control.lock().await;
        if let Some(error) = self.snapshot().error {
            return Err(error);
        }
        if self.readers.load(Ordering::Acquire) >= 8 {
            return Err(TorrentError::Engine);
        }
        let session = self.session.upgrade().ok_or(TorrentError::Cancelled)?;
        session
            .update_only_files(&self.torrent, &HashSet::new())
            .await
            .map_err(|_| TorrentError::Engine)?;
        let live = !self.torrent.is_paused();
        let reader = self
            .torrent
            .clone()
            .stream(index)
            .await
            .map_err(|_| TorrentError::Engine)?;
        self.resume(&session, true).await?;
        self.readers.fetch_add(1, Ordering::AcqRel);
        if live {
            self.refresh.store(true, Ordering::Release);
        }
        *lock(&self.read_files).entry(index).or_default() += 1;
        lock(&self.activity).last_progress = Instant::now();
        Ok(Reader {
            inner: Box::pin(reader),
            entry: self.clone(),
            index,
        })
    }
}

pub(super) trait MediaRead: tokio::io::AsyncRead + tokio::io::AsyncSeek + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncSeek + Send> MediaRead for T {}

pub(super) struct Reader {
    pub inner: std::pin::Pin<Box<dyn MediaRead>>,
    pub entry: Arc<Entry>,
    index: usize,
}

impl Drop for Reader {
    fn drop(&mut self) {
        let mut files = lock(&self.entry.read_files);
        if let Some(count) = files.get_mut(&self.index) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                files.remove(&self.index);
            }
        }
        if self.entry.readers.fetch_sub(1, Ordering::AcqRel) > 1 {
            self.entry.refresh.store(true, Ordering::Release);
        }
    }
}

pub(super) struct Playback {
    pub entry: Arc<Entry>,
    pub index: usize,
    pub size: u64,
    pub stop: CancellationToken,
}

/// An open playback lease. Close explicitly to await torrent pausing.
pub struct TorrentStream {
    /// Loopback URL with a random 128-bit bearer token; treat it as a secret.
    pub url: String,
    /// Selected torrent file's display name.
    pub file_name: String,
    /// Logical media length in bytes, independent of cache occupancy.
    pub file_size: u64,
    pub(super) playback: Arc<Playback>,
    pub(super) token: String,
    pub(super) host: Weak<Host>,
}

impl TorrentStream {
    /// Current peer, transfer, availability, and terminal-error snapshot.
    pub fn stats(&self) -> TorrentStats {
        self.playback.entry.snapshot()
    }

    /// Invalidate the URL and readers, release the lease, and await pausing.
    pub async fn close(&self) -> Result<(), TorrentError> {
        self.playback.stop.cancel();
        if let Some(host) = self.host.upgrade() {
            if host.shared.stop.is_cancelled() {
                return Ok(());
            }
            let shared = host.shared.clone();
            let token = self.token.clone();
            host.handle
                .spawn(async move { shared.release(&token).await })
                .await
                .map_err(|_| TorrentError::Engine)?;
        }
        Ok(())
    }
}

impl Drop for TorrentStream {
    fn drop(&mut self) {
        self.playback.stop.cancel();
    }
}
