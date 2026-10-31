use super::{
    health::LinkHealth,
    host::Command,
    model::PlaybackError,
    selection::PlaybackSelection,
    source::{self, ResolvedSource},
};
use futures::future::BoxFuture;
use panorama_core::{
    media::{
        MediaError,
        policy::BufferingSample,
        proxy::{MediaProxy, ProxyOptions, SessionHandle},
    },
    torrent::{
        CancellationToken, TorrentEngine, TorrentError, TorrentOptions, TorrentStats, TorrentStream,
    },
};
use panorama_mpv::{
    CacheState, Mpv, MpvLibrary, Player, PlayerEvent, PlayerOptions, RawWindow, SeekMode,
};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Duration,
};
use tokio::sync::{Mutex, OnceCell};
#[path = "backend_playback.rs"]
mod playback;
use playback::{NativePlayback, Transport};

pub trait Backend: Send + Sync + 'static {
    fn resolve(
        self: Arc<Self>,
        selection: PlaybackSelection,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<Box<dyn Playback>, PlaybackError>>;
    fn shutdown(&self) -> BoxFuture<'_, ()>;
    fn link_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(1)
    }
}

pub trait Playback: Send {
    fn load(
        &mut self,
        window: Option<RawWindow>,
        volume: u8,
        muted: bool,
        paused: bool,
        start: Option<f64>,
    ) -> Result<(), PlaybackError>;
    fn command(&mut self, command: &Command) -> Result<(), PlaybackError>;
    fn events(&mut self) -> Vec<PlayerEvent>;
    fn cache(&self, cache: &CacheState, now_ms: u64, duration: Option<f64>) -> Option<LinkHealth>;
    fn error(&self) -> Option<PlaybackError>;
    /// The torrent's stats, for a torrent source.
    fn torrent(&self) -> Option<TorrentStats> {
        None
    }
    fn stop(&mut self);
    fn close_session(&mut self) -> BoxFuture<'_, ()>;
    fn close_player(self: Box<Self>) -> BoxFuture<'static, ()>;
}

pub struct NativeBackend {
    directory: PathBuf,
    file: Option<PathBuf>,
    diagnostics: bool,
    library: OnceCell<MpvLibrary>,
    proxy: Mutex<Option<MediaProxy>>,
    torrent: OnceCell<Arc<TorrentEngine>>,
}

impl NativeBackend {
    pub fn new(directory: PathBuf, file: Option<PathBuf>, diagnostics: bool) -> Self {
        Self {
            directory,
            file,
            diagnostics,
            library: OnceCell::new(),
            proxy: Mutex::new(None),
            torrent: OnceCell::new(),
        }
    }

    async fn library(&self) -> Result<MpvLibrary, PlaybackError> {
        self.library
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(|| {
                    let directory = std::env::var_os("PANORAMA_LIBMPV_DIR").map(PathBuf::from);
                    Mpv::load_library(directory.as_deref())
                })
                .await
                .map_err(|_| PlaybackError::LibraryMissing)?
                .map_err(|_| PlaybackError::LibraryMissing)
            })
            .await
            .cloned()
    }
}

impl Backend for NativeBackend {
    fn resolve(
        self: Arc<Self>,
        selection: PlaybackSelection,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<Box<dyn Playback>, PlaybackError>> {
        Box::pin(async move {
            let resolved = if self.file.is_none() {
                Some(source::resolve(
                    selection
                        .selected()
                        .ok_or(PlaybackError::SourceUnavailable)?,
                )?)
            } else {
                None
            };
            let library = self.library().await?;
            if cancel.is_cancelled() {
                return Err(PlaybackError::SourceUnavailable);
            }
            let (url, transport) = if let Some(file) = &self.file {
                let path = file.clone();
                let path = tokio::task::spawn_blocking(move || path.canonicalize())
                    .await
                    .map_err(|_| PlaybackError::SourceUnavailable)?
                    .map_err(|_| PlaybackError::SourceUnavailable)?;
                (path.to_string_lossy().into_owned(), Transport::File)
            } else {
                match resolved.ok_or(PlaybackError::SourceUnavailable)? {
                    ResolvedSource::Http(source) => {
                        let mut slot = self.proxy.lock().await;
                        if slot.is_none() {
                            let mut options = ProxyOptions::default();
                            options.cache.directory = self.directory.join("media-cache");
                            *slot = Some(
                                MediaProxy::start(options, None)
                                    .await
                                    .map_err(media_error)?,
                            );
                        }
                        if cancel.is_cancelled() {
                            return Err(PlaybackError::SourceUnavailable);
                        }
                        let session = slot
                            .as_mut()
                            .ok_or(PlaybackError::Network)?
                            .create_session(source)
                            .await
                            .map_err(media_error)?;
                        (session.url.clone(), Transport::Http(session))
                    }
                    ResolvedSource::Torrent(source) => {
                        let engine = self
                            .torrent
                            .get_or_init(|| async {
                                Arc::new(TorrentEngine::new(TorrentOptions {
                                    cache_dir: self.directory.join("torrent-cache"),
                                    ..Default::default()
                                }))
                            })
                            .await;
                        let stream = engine
                            .open(source, cancel.clone())
                            .await
                            .map_err(torrent_error)?;
                        (stream.url.clone(), Transport::Torrent(stream))
                    }
                }
            };
            let fonts = self.directory.join("fonts");
            let fonts = tokio::task::spawn_blocking(move || {
                crate::assets::write_fonts(&fonts).map(|()| fonts)
            })
            .await
            .ok()
            .and_then(Result::ok);
            let playback: Box<dyn Playback> = Box::new(NativePlayback {
                library,
                fonts,
                url,
                transport,
                player: None,
                events: None,
                cancel,
                shutdown: false,
                diagnostics: self.diagnostics,
                pending_seek: None,
                reported_time: false,
                close_started: None,
                subtitles_due: Vec::new(),
                file_loaded: false,
            });
            Ok(playback)
        })
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {
            if let Some(mut proxy) = self.proxy.lock().await.take() {
                proxy.close().await;
            }
            if let Some(engine) = self.torrent.get() {
                let _ = engine.shutdown().await;
            }
        })
    }
}

fn media_error(error: MediaError) -> PlaybackError {
    match error {
        MediaError::Status(401 | 403 | 404 | 410)
        | MediaError::ResolverFailed
        | MediaError::ResolveLimit
        | MediaError::Representation(_) => PlaybackError::SourceExpired,
        MediaError::InvalidDestination => PlaybackError::SourceUnavailable,
        _ => PlaybackError::Network,
    }
}

pub(super) fn torrent_error(error: TorrentError) -> PlaybackError {
    match error {
        TorrentError::NoPeers | TorrentError::MetadataTimeout => PlaybackError::NoPeers,
        TorrentError::NoVideoFile => PlaybackError::NoVideoFile,
        TorrentError::Disk(_) => PlaybackError::TorrentCache,
        TorrentError::InvalidSource => PlaybackError::SourceUnavailable,
        _ => PlaybackError::Network,
    }
}

pub(super) async fn close(mut playback: Box<dyn Playback>) {
    playback.stop();
    playback.close_session().await;
    playback.close_player().await;
}
