use super::{
    backend::{Backend, Playback},
    health::LinkHealth,
    host::Command,
    model::PlaybackError,
    selection::PlaybackSelection,
};
use futures::future::BoxFuture;
use panorama_core::torrent::CancellationToken;
use panorama_mpv::{CacheState, PlayerEvent, RawWindow, Track, TrackKind};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureState {
    Preparing,
    Loading,
    Buffering,
    Error,
    Ended,
    Ready,
    /// Buffering with a slow link, so the wait estimate shows.
    Waiting,
    /// Ready after a stall on a link slower than the source.
    Heavy,
}

impl FixtureState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "waiting" => Some(Self::Waiting),
            "heavy" => Some(Self::Heavy),
            "preparing" => Some(Self::Preparing),
            "loading" => Some(Self::Loading),
            "buffering" => Some(Self::Buffering),
            "error" => Some(Self::Error),
            "ended" => Some(Self::Ended),
            "ready" => Some(Self::Ready),
            _ => None,
        }
    }
}

pub struct Script {
    pub delay: Duration,
    pub ignore_cancel: bool,
    pub error: Option<PlaybackError>,
    pub events: Vec<PlayerEvent>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            delay: Duration::ZERO,
            ignore_cancel: false,
            error: None,
            events: vec![
                PlayerEvent::Duration(Some(600.0)),
                PlayerEvent::TimePos(Some(1.0)),
            ],
        }
    }
}

pub struct FakeBackend {
    state: Option<FixtureState>,
    pub scripts: Mutex<VecDeque<Script>>,
    pub log: Arc<Mutex<Vec<String>>>,
}

impl FakeBackend {
    pub fn new(state: Option<FixtureState>) -> Self {
        Self {
            state,
            scripts: Mutex::new(VecDeque::new()),
            log: Arc::default(),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Backend for FakeBackend {
    fn resolve(
        self: Arc<Self>,
        _selection: PlaybackSelection,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<Box<dyn Playback>, PlaybackError>> {
        Box::pin(async move {
            if self.state == Some(FixtureState::Preparing) {
                cancel.cancelled().await;
                return Err(PlaybackError::SourceUnavailable);
            }
            let script = lock(&self.scripts).pop_front().unwrap_or_default();
            if script.ignore_cancel {
                tokio::time::sleep(script.delay).await;
            } else {
                tokio::select! {
                    _ = tokio::time::sleep(script.delay) => {},
                    _ = cancel.cancelled() => return Err(PlaybackError::SourceUnavailable),
                }
            }
            if let Some(error) = script.error {
                return Err(error);
            }
            let mut events = script.events;
            if let Some(state) = self.state {
                events = match state {
                    FixtureState::Loading => vec![],
                    FixtureState::Error => {
                        vec![PlayerEvent::Error(panorama_mpv::MpvError::Loading)]
                    }
                    FixtureState::Preparing => vec![],
                    _ => vec![
                        PlayerEvent::Duration(Some(6100.0)),
                        PlayerEvent::TimePos(Some(1200.0)),
                        PlayerEvent::TrackList(fixture_tracks()),
                        PlayerEvent::DemuxerCacheState(CacheState {
                            buffered_seconds: Some(180.0),
                            cache_end_seconds: Some(1380.0),
                            ..Default::default()
                        }),
                    ],
                };
                match state {
                    FixtureState::Buffering | FixtureState::Waiting => {
                        events.push(PlayerEvent::PausedForCache(true));
                    }
                    FixtureState::Ended => events.push(PlayerEvent::EofReached(true)),
                    FixtureState::Heavy => {
                        // A stall only counts once this load has moved.
                        events.push(PlayerEvent::TimePos(Some(1201.0)));
                        events.push(PlayerEvent::PausedForCache(true));
                        events.push(PlayerEvent::PausedForCache(false));
                    }
                    _ => {}
                }
                if matches!(state, FixtureState::Waiting | FixtureState::Heavy) {
                    let buffered = if state == FixtureState::Waiting {
                        0.0
                    } else {
                        1.0
                    };
                    let samples = if state == FixtureState::Waiting {
                        3
                    } else {
                        10
                    };
                    events.extend((0..samples).map(|_| {
                        PlayerEvent::DemuxerCacheState(CacheState {
                            buffered_seconds: Some(buffered),
                            cache_end_seconds: Some(1200.0 + buffered),
                            ..Default::default()
                        })
                    }));
                }
            }
            let link = matches!(
                self.state,
                Some(FixtureState::Waiting | FixtureState::Heavy)
            )
            .then_some(LinkHealth {
                source_mbps: Some(8.4),
                download_mbps: Some(2.1),
                transfer_demanded: Some(true),
            });
            let playback: Box<dyn Playback> = Box::new(FakePlayback {
                events,
                link,
                log: self.log.clone(),
            });
            Ok(playback)
        })
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }

    fn link_interval(&self) -> Duration {
        if self.state.is_some() {
            Duration::ZERO
        } else {
            Duration::from_secs(1)
        }
    }
}

fn track(id: i64, kind: TrackKind, title: &str, lang: &str, codec: &str, selected: bool) -> Track {
    Track {
        id,
        kind,
        title: Some(title.into()),
        lang: Some(lang.into()),
        codec: Some(codec.into()),
        default: selected,
        forced: false,
        external: false,
        hearing_impaired: false,
        selected,
    }
}

fn fixture_tracks() -> Vec<Track> {
    vec![
        track(1, TrackKind::Audio, "English", "eng", "eac3 5.1", true),
        track(
            2,
            TrackKind::Audio,
            "English Commentary",
            "eng",
            "aac stereo",
            false,
        ),
        track(3, TrackKind::Audio, "French", "fra", "ac3 5.1", false),
        track(4, TrackKind::Audio, "Japanese", "jpn", "aac stereo", false),
        track(5, TrackKind::Subtitle, "English", "eng", "subrip", false),
    ]
}

struct FakePlayback {
    events: Vec<PlayerEvent>,
    link: Option<LinkHealth>,
    log: Arc<Mutex<Vec<String>>>,
}

impl Playback for FakePlayback {
    fn load(&mut self, _: Option<RawWindow>, _: u8, _: bool, _: bool) -> Result<(), PlaybackError> {
        lock(&self.log).push("load".into());
        Ok(())
    }

    fn command(&mut self, command: &Command) -> Result<(), PlaybackError> {
        let label = match command {
            Command::SetPaused(paused) => format!("pause:{paused}"),
            Command::Seek(seconds) => format!("seek:{seconds}"),
            Command::SeekRelative(seconds) => format!("relative:{seconds}"),
            Command::SetVolume(volume) => format!("volume:{volume}"),
            Command::SetMuted(muted) => format!("mute:{muted}"),
            Command::SelectAudio(id) => format!("audio:{id}"),
            Command::SelectSubtitle(id) => format!("subtitle:{id:?}"),
            _ => "command".into(),
        };
        lock(&self.log).push(label);
        Ok(())
    }

    fn events(&mut self) -> Vec<PlayerEvent> {
        std::mem::take(&mut self.events)
    }
    fn cache(&self, _: &CacheState, _: u64, _: Option<f64>) -> Option<LinkHealth> {
        self.link
    }
    fn error(&self) -> Option<PlaybackError> {
        None
    }
    fn stop(&mut self) {
        lock(&self.log).push("mpv stop".into());
    }
    fn close_session(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async {
            lock(&self.log).push("session close".into());
        })
    }
    fn close_player(self: Box<Self>) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            lock(&self.log).push("player close".into());
        })
    }
}
