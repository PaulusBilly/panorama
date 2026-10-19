use super::{
    backend::{Backend, Playback},
    host::Command,
    model::PlaybackError,
    selection::PlaybackSelection,
};
use futures::future::BoxFuture;
use panorama_core::torrent::CancellationToken;
use panorama_mpv::{CacheState, PlayerEvent, RawWindow};
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
}

impl FixtureState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
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
                        PlayerEvent::Duration(Some(600.0)),
                        PlayerEvent::TimePos(Some(100.0)),
                    ],
                };
                match state {
                    FixtureState::Buffering => events.push(PlayerEvent::PausedForCache(true)),
                    FixtureState::Ended => events.push(PlayerEvent::EofReached(true)),
                    _ => {}
                }
            }
            let playback: Box<dyn Playback> = Box::new(FakePlayback {
                events,
                log: self.log.clone(),
            });
            Ok(playback)
        })
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

struct FakePlayback {
    events: Vec<PlayerEvent>,
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
    fn cache(&self, _: &CacheState, _: u64, _: Option<f64>) {}
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
