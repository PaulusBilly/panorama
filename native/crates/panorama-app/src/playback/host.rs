use super::{
    backend::{Backend, NativeBackend, Playback, close},
    fake::{FakeBackend, FixtureState},
    model::{HostEvent, PlaybackError, PlaybackModel, Status, TorrentHealth},
    progress::{CoreProgress, Progress, ProgressSink},
    selection::{PlaybackSelection, SourceRef, StartMode},
    subtitle_render::SubtitleSetup,
};
use crate::services::Services;
use futures::{StreamExt, channel::mpsc, future::BoxFuture, stream::FuturesUnordered};
use panorama_core::{stremio::PlaybackSample, torrent::CancellationToken};
use panorama_mpv::{PlayerEvent, RawWindow};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::time::Instant;

pub enum Command {
    Start {
        window: RawWindow,
        selection: Box<PlaybackSelection>,
    },
    SwitchSource(SourceRef),
    /// Replace the addon groups while Film still delivers sources.
    Groups(Vec<panorama_core::addons::StreamGroup>),
    Retry,
    /// "Keep waiting" on a torrent with no peers: close the stream and open it again, which
    /// is the engine's retry (it only clears a latched `NoPeers` once no lease is open).
    RetryTorrent,
    SetPaused(bool),
    TogglePause,
    Seek(f64),
    SeekRelative(f64),
    SetVolume(u8),
    SetMuted(bool),
    SelectAudio(i64),
    SelectSubtitle(Option<i64>),
    /// Subtitle preferences and which renderer draws the selected track.
    Subtitles(Box<SubtitleSetup>),
    /// Audio and video output properties (`audio-device`, `audio-channels`, ...).
    Output(Vec<(&'static str, String)>),
    Stop,
    Attach(mpsc::UnboundedSender<HostEvent>),
    Shutdown(tokio::sync::oneshot::Sender<()>),
    #[cfg(test)]
    Headless(Box<PlaybackSelection>),
}

pub struct PlaybackHost {
    commands: tokio::sync::mpsc::UnboundedSender<Command>,
}

impl PlaybackHost {
    pub fn new(
        services: Arc<Services>,
        fixture: Option<FixtureState>,
        file: Option<PathBuf>,
    ) -> Self {
        let backend: Arc<dyn Backend> =
            if services.addons.is_none() && file.is_none() && fixture.is_some() {
                Arc::new(FakeBackend::new(fixture))
            } else {
                Arc::new(NativeBackend::new(
                    services.data_dir.clone(),
                    file,
                    services.addons.is_none(),
                ))
            };
        Self::with_backend(
            &services.runtime,
            backend,
            Arc::new(CoreProgress(services.clone())),
        )
    }

    pub fn with_backend(
        runtime: &tokio::runtime::Handle,
        backend: Arc<dyn Backend>,
        progress: Arc<dyn ProgressSink>,
    ) -> Self {
        let (commands, incoming) = tokio::sync::mpsc::unbounded_channel();
        let (events, receiver) = mpsc::unbounded();
        runtime.spawn(Runner::new(backend, progress, events).run(incoming));
        drop(receiver);
        Self { commands }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn shutdown(&self) -> tokio::sync::oneshot::Receiver<()> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.send(Command::Shutdown(sender));
        receiver
    }

    pub fn events(&mut self) -> Option<mpsc::UnboundedReceiver<HostEvent>> {
        let (sender, receiver) = mpsc::unbounded();
        self.send(Command::Attach(sender));
        Some(receiver)
    }
}

impl Drop for PlaybackHost {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
    }
}

type Opening = BoxFuture<'static, (u64, Result<Box<dyn Playback>, super::model::PlaybackError>)>;

struct Runner {
    backend: Arc<dyn Backend>,
    progress: Arc<dyn ProgressSink>,
    events: mpsc::UnboundedSender<HostEvent>,
    selection: Option<PlaybackSelection>,
    window: Option<RawWindow>,
    active: Option<Box<dyn Playback>>,
    pending: FuturesUnordered<Opening>,
    cancel: Option<CancellationToken>,
    generation: u64,
    model: PlaybackModel,
    resume: Option<f64>,
    awaiting_resume: Option<f64>,
    position: f64,
    begun: bool,
    last_presence: Option<Instant>,
    last_link: Option<Instant>,
    /// When the current torrent source started.
    torrent_since: Option<Instant>,
    link_interval: Duration,
    origin: Instant,
    /// The latest subtitle and output settings, replayed onto every new player.
    subtitles: Option<SubtitleSetup>,
    output: Option<Vec<(&'static str, String)>>,
}

impl Runner {
    fn new(
        backend: Arc<dyn Backend>,
        progress: Arc<dyn ProgressSink>,
        events: mpsc::UnboundedSender<HostEvent>,
    ) -> Self {
        let link_interval = backend.link_interval();
        Self {
            backend,
            progress,
            events,
            selection: None,
            window: None,
            active: None,
            pending: FuturesUnordered::new(),
            cancel: None,
            generation: 0,
            model: PlaybackModel::new(String::new(), SourceRef::default()),
            resume: None,
            awaiting_resume: None,
            position: 0.0,
            begun: false,
            last_presence: None,
            last_link: None,
            torrent_since: None,
            link_interval,
            origin: Instant::now(),
            subtitles: None,
            output: None,
        }
    }

    fn emit(&mut self, event: HostEvent) {
        if let HostEvent::Failed(error) = &event {
            crate::debug::log(format!("player error={error:?}"));
        }
        self.model.apply(event.clone());
        let _ = self.events.unbounded_send(event);
    }

    async fn stop(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(active) = self.active.take() {
            close(active).await;
        }
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        if self.begun
            && let Some(selection) = &self.selection
        {
            self.progress
                .send(Progress::Stop(selection.film.id.clone()))
                .await;
        }
        self.begun = false;
        self.resume = None;
        self.awaiting_resume = None;
        self.progress.clear();
        self.last_presence = None;
        self.torrent_since = None;
    }

    async fn start(
        &mut self,
        selection: PlaybackSelection,
        window: Option<RawWindow>,
        continuing: bool,
    ) {
        self.stop().await;
        if !continuing {
            self.model = PlaybackModel::new(selection.film.name.clone(), selection.source);
            self.emit(HostEvent::Closed);
            self.emit(HostEvent::Volume(100));
            self.emit(HostEvent::Muted(false));
        }
        self.window = window;
        self.position = selection.start.seconds();
        self.resume = match selection.start {
            StartMode::Resume(seconds) => Some(seconds),
            StartMode::Restart => None,
        };
        self.awaiting_resume = self.resume;
        self.emit(HostEvent::SourceChanged(selection.source));
        self.emit(HostEvent::Resolving);
        if selection
            .selected()
            .is_some_and(super::torrent_ui::is_torrent)
        {
            self.torrent_since = Some(Instant::now());
            self.emit(HostEvent::Torrent(
                super::torrent_ui::TorrentHealth::connecting(),
            ));
        }
        self.selection = Some(selection.clone());
        let generation = self.generation;
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        let backend = self.backend.clone();
        self.pending.push(Box::pin(async move {
            (generation, backend.resolve(selection, cancel).await)
        }));
    }

    async fn opened(
        &mut self,
        generation: u64,
        result: Result<Box<dyn Playback>, super::model::PlaybackError>,
    ) {
        if generation != self.generation {
            if let Ok(active) = result {
                close(active).await;
            }
            return;
        }
        match result {
            Ok(mut active) => {
                self.emit(HostEvent::Loading);
                let result = active.load(
                    self.window.clone(),
                    self.model.volume,
                    self.model.muted,
                    self.model.paused,
                );
                self.active = Some(active);
                match result {
                    Err(error) => self.emit(HostEvent::Failed(error)),
                    Ok(()) => self.replay_settings(),
                }
            }
            Err(error) => self.emit(HostEvent::Failed(error)),
        }
    }

    /// Apply the remembered subtitle and output settings to the player just loaded. A refused
    /// property never stops playback.
    fn replay_settings(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        if let Some(output) = &self.output {
            let _ = active.command(&Command::Output(output.clone()));
        }
        if let Some(setup) = &self.subtitles {
            let _ = active.command(&Command::Subtitles(Box::new(setup.clone())));
        }
    }

    async fn control(&mut self, command: Command) {
        match command {
            Command::Subtitles(setup) => {
                self.subtitles = Some((*setup).clone());
                if let Some(active) = &mut self.active {
                    let _ = active.command(&Command::Subtitles(setup));
                }
            }
            Command::Output(properties) => {
                if let Some(active) = &mut self.active {
                    let _ = active.command(&Command::Output(properties.clone()));
                }
                self.output = Some(properties);
            }
            Command::Attach(sender) => self.events = sender,
            Command::Shutdown(done) => {
                self.stop().await;
                while let Some((_, result)) = self.pending.next().await {
                    if let Ok(active) = result {
                        close(active).await;
                    }
                }
                self.backend.shutdown().await;
                self.selection = None;
                self.window = None;
                self.emit(HostEvent::Closed);
                let _ = done.send(());
            }
            Command::Start { window, selection } => {
                self.start(*selection, Some(window), false).await
            }
            #[cfg(test)]
            Command::Headless(selection) => self.start(*selection, None, false).await,
            Command::SwitchSource(source) => {
                if let Some(mut selection) = self.selection.clone() {
                    selection.source = source;
                    if selection.selected().is_none() {
                        return;
                    }
                    selection.start = StartMode::Resume(self.position);
                    self.start(selection, self.window.clone(), true).await;
                }
            }
            Command::Groups(groups) => {
                if let Some(selection) = &mut self.selection {
                    selection.groups = groups;
                }
            }
            Command::Retry => {
                if let Some(mut selection) = self.selection.clone() {
                    selection.start = StartMode::Resume(self.position);
                    self.start(selection, self.window.clone(), true).await;
                }
            }
            Command::RetryTorrent => {
                if self.model.status == Status::NoPeers
                    && let Some(mut selection) = self.selection.clone()
                {
                    selection.start = StartMode::Resume(self.position);
                    self.start(selection, self.window.clone(), true).await;
                }
            }
            Command::Stop => {
                self.stop().await;
                self.selection = None;
                self.window = None;
                self.emit(HostEvent::Closed);
            }
            command => self.player_command(command).await,
        }
    }

    async fn run(mut self, mut incoming: tokio::sync::mpsc::UnboundedReceiver<Command>) {
        let mut poll = tokio::time::interval(Duration::from_millis(10));
        let mut stats = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                command = incoming.recv() => match command {
                    Some(command) => self.control(command).await,
                    None => break,
                },
                Some((generation, result)) = self.pending.next(), if !self.pending.is_empty() => self.opened(generation, result).await,
                _ = poll.tick() => self.tick().await,
                _ = stats.tick() => self.poll_stats(),
            }
        }
        self.stop().await;
        while let Some((_, result)) = self.pending.next().await {
            if let Ok(active) = result {
                close(active).await;
            }
        }
        self.backend.shutdown().await;
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;

#[path = "host_events.rs"]
mod events;
