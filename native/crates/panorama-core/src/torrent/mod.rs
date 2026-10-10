//! In-process torrent playback over authenticated loopback HTTP.
mod cache;
mod directory;
mod engine;
mod opening;
mod runtime;
mod server;
mod source;
mod stream;

pub use engine::TorrentEngine;
pub use source::TorrentSource;
pub use stream::TorrentStream;
pub use tokio_util::sync::CancellationToken;

use std::{
    fmt,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

/// Cache and network settings; constructing an engine performs no I/O.
#[derive(Clone, Debug)]
pub struct TorrentOptions {
    /// Dedicated, initially absent directory owned and removed by this engine.
    pub cache_dir: PathBuf,
    /// Maximum allocated cache data bytes, shared by all torrents.
    pub max_cache_bytes: u64,
    /// Stop the library session after this interval without open streams.
    pub idle_stop_after: Duration,
    /// Maximum metadata and initialization wait.
    pub metadata_timeout: Duration,
    /// Deadline from open's start for first payload, then maximum
    /// sustained peerless lack of progress while HTTP readers are active.
    pub no_peers_timeout: Duration,
    /// Upload bytes per second; `None` is unlimited. Valid range: 1..=u32::MAX.
    pub upload_limit_bytes_per_sec: Option<u64>,
    /// Optional incoming TCP port (0 chooses a port); UPnP is always disabled.
    pub listen_port: Option<u16>,
}

impl Default for TorrentOptions {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("panorama-torrent-cache"),
            max_cache_bytes: 10 * 1024 * 1024 * 1024,
            idle_stop_after: Duration::from_secs(300),
            metadata_timeout: Duration::from_secs(45),
            no_peers_timeout: Duration::from_secs(60),
            upload_limit_bytes_per_sec: Some(1024 * 1024),
            listen_port: None,
        }
    }
}

/// Disk failure exposed without paths or tracker credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskError {
    /// The configured cap or available free space cannot accommodate a write.
    Full,
    /// A filesystem operation failed.
    Io,
}

/// Redacted, stable outcomes suitable for player UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TorrentError {
    /// Metadata or initialization did not finish before its deadline.
    MetadataTimeout,
    /// No first payload, or sustained peerless lack of progress, before deadline.
    NoPeers,
    /// Metadata contains no recognized video file.
    NoVideoFile,
    /// Invalid hash, file index, or options.
    InvalidSource,
    /// Caller cancelled, stream closed, or engine stopped.
    Cancelled,
    /// Cache allocation or filesystem error.
    Disk(DiskError),
    /// The library or HTTP/runtime setup failed.
    Engine,
}

impl fmt::Display for TorrentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MetadataTimeout => "torrent metadata timed out",
            Self::NoPeers => "torrent has no available payload/peers within the deadline",
            Self::NoVideoFile => "torrent contains no video file",
            Self::InvalidSource => "invalid torrent source or options",
            Self::Cancelled => "torrent operation cancelled",
            Self::Disk(DiskError::Full) => "torrent cache is full",
            Self::Disk(DiskError::Io) => "torrent cache I/O failed",
            Self::Engine => "torrent engine failed",
        })
    }
}
impl std::error::Error for TorrentError {}
impl From<std::io::Error> for TorrentError {
    fn from(value: std::io::Error) -> Self {
        Self::Disk(if value.kind() == std::io::ErrorKind::StorageFull {
            DiskError::Full
        } else {
            DiskError::Io
        })
    }
}

/// State for the player's source label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TorrentState {
    /// Waiting for peers or the first verified payload.
    Connecting,
    /// Checking resolved torrent metadata and existing pieces.
    Metadata,
    /// Verified media is available.
    Streaming,
    /// Peerless timeout, disk failure, or library failure; inspect `error`.
    Stalled,
}

/// Snapshot of torrent activity; counters include redundant received payload.
#[derive(Clone, Debug)]
pub struct TorrentStats {
    /// Live peer connections.
    pub peers_live: u32,
    /// Connections being established.
    pub peers_connecting: u32,
    /// Received torrent payload, including unverified or duplicate transfers.
    pub downloaded_bytes: u64,
    /// Verified selected-file progress (piece boundaries may overlap files).
    pub verified_bytes: u64,
    /// Rolling download bytes per second.
    pub download_bps: u64,
    /// Rolling upload bytes per second.
    pub upload_bps: u64,
    /// Playback discovery/availability state.
    pub state: TorrentState,
    /// Terminal playback failure, including `Disk(Full)` and `NoPeers`.
    pub error: Option<TorrentError>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests;
