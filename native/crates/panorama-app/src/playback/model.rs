pub use super::selection::SourceRef;
pub use super::torrent_ui::TorrentHealth;
use panorama_mpv::{EndReason, MpvError, PlayerEvent, TrackKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    ResolvingSource,
    LoadingVideo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackError {
    LibraryMissing,
    SourceUnavailable,
    SourceExpired,
    NoPeers,
    NoVideoFile,
    /// The torrent cache could not be used (disk full or unwritable).
    TorrentCache,
    Network,
    Decode,
    Unknown,
}

impl PlaybackError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::LibraryMissing => "Playback components are missing. Reinstall Panorama.",
            Self::SourceUnavailable => "This source could not be played. Try another source.",
            Self::SourceExpired => "This source has expired. Try another source.",
            Self::NoPeers => "No one is sharing this torrent right now. Try another source.",
            Self::NoVideoFile => "This source does not contain a video. Try another source.",
            Self::TorrentCache => {
                "Panorama couldn't use its torrent cache. Free some disk space or restart Panorama."
            }
            Self::Network => "The connection dropped. Check your connection and try again.",
            Self::Decode => "This video could not be played. Try another source.",
            Self::Unknown => "Playback stopped unexpectedly. Try again.",
        }
    }
}

impl std::fmt::Display for PlaybackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for PlaybackError {}

impl From<MpvError> for PlaybackError {
    fn from(error: MpvError) -> Self {
        match error {
            MpvError::LibraryUnavailable
            | MpvError::MissingSymbol
            | MpvError::IncompatibleApi
            | MpvError::UnsupportedPlatform => Self::LibraryMissing,
            MpvError::Loading => Self::SourceUnavailable,
            MpvError::NothingToPlay => Self::NoVideoFile,
            MpvError::Demuxer | MpvError::Output => Self::Decode,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Idle,
    Preparing(Stage),
    Ready,
    Buffering,
    Ended,
    /// A torrent nobody is sharing; the player offers another source or more waiting.
    NoPeers,
    Error(PlaybackError),
}

#[derive(Clone, PartialEq)]
pub struct Track {
    pub id: i64,
    pub label: String,
    pub description: Option<String>,
    pub lang: Option<String>,
    pub codec: Option<String>,
    pub selected: bool,
    /// Added from a file (an addon's subtitles), not part of the video.
    pub external: bool,
}

impl std::fmt::Debug for Track {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Track")
            .field("id", &self.id)
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq)]
pub struct PlaybackModel {
    pub status: Status,
    /// The host moved on from a failed or stuck source by itself; cleared by any source change.
    pub falling_back: bool,
    pub has_reached_playable: bool,
    pub title: String,
    pub time: Option<f64>,
    pub duration: Option<f64>,
    pub paused: bool,
    pub volume: u8,
    pub muted: bool,
    pub buffered_until: Option<f64>,
    /// Media buffered ahead of the playhead (`demuxer-cache-duration`).
    pub cache_seconds: Option<f64>,
    /// Stalls after playback first became playable.
    pub rebuffers: u32,
    pub audio: Vec<Track>,
    pub subtitles: Vec<Track>,
    /// The cue now showing, sanitized (`sub-text`).
    pub cue: Option<String>,
    /// Audio outputs mpv reported.
    pub devices: Vec<super::output::Device>,
    pub source: SourceRef,
    pub hwdec: Option<String>,
    pub fullscreen: bool,
    /// Set from the start for a torrent source, then updated from the engine's stats.
    pub torrent: Option<TorrentHealth>,
    known_file: bool,
    eof_reached: bool,
    /// This load has advanced its position at least once.
    moved: bool,
    /// Where the last seek landed, until playback moves on after it.
    seek_from: Option<f64>,
}

#[derive(Clone, PartialEq)]
pub enum HostEvent {
    Resolving,
    Loading,
    Failed(PlaybackError),
    Mpv(PlayerEvent),
    SourceChanged(SourceRef),
    /// The host is trying the next ranked source after the previous one failed or stuck.
    FallingBack,
    Closed,
    Volume(u8),
    Muted(bool),
    Seeked(f64),
    Link(super::health::LinkHealth),
    Torrent(TorrentHealth),
}

impl std::fmt::Debug for HostEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mpv(event) => f
                .debug_tuple("Mpv")
                .field(&std::mem::discriminant(event))
                .finish(),
            Self::Failed(error) => f.debug_tuple("Failed").field(error).finish(),
            Self::SourceChanged(source) => f.debug_tuple("SourceChanged").field(source).finish(),
            Self::Resolving => f.write_str("Resolving"),
            Self::Loading => f.write_str("Loading"),
            Self::FallingBack => f.write_str("FallingBack"),
            Self::Closed => f.write_str("Closed"),
            Self::Volume(volume) => f.debug_tuple("Volume").field(volume).finish(),
            Self::Muted(muted) => f.debug_tuple("Muted").field(muted).finish(),
            Self::Seeked(seconds) => f.debug_tuple("Seeked").field(seconds).finish(),
            Self::Link(_) => f.write_str("Link"),
            Self::Torrent(health) => f.debug_tuple("Torrent").field(health).finish(),
        }
    }
}

impl PlaybackModel {
    pub fn new(title: String, source: SourceRef) -> Self {
        Self {
            status: Status::Idle,
            falling_back: false,
            has_reached_playable: false,
            title,
            time: None,
            duration: None,
            paused: false,
            volume: 100,
            muted: false,
            buffered_until: None,
            cache_seconds: None,
            rebuffers: 0,
            audio: vec![],
            subtitles: vec![],
            cue: None,
            devices: vec![],
            source,
            hwdec: None,
            fullscreen: false,
            torrent: None,
            known_file: false,
            eof_reached: false,
            moved: false,
            seek_from: None,
        }
    }

    pub fn apply(&mut self, event: HostEvent) {
        match event {
            HostEvent::Resolving => {
                self.status = Status::Preparing(Stage::ResolvingSource);
                self.known_file = false;
                self.eof_reached = false;
                self.paused = false;
                self.time = None;
                self.duration = None;
                self.buffered_until = None;
                self.cache_seconds = None;
                self.rebuffers = 0;
                self.moved = false;
                self.seek_from = None;
                self.audio.clear();
                self.subtitles.clear();
                self.cue = None;
                self.hwdec = None;
                self.torrent = None;
            }
            HostEvent::Loading => {
                self.known_file = true;
                self.status = Status::Preparing(Stage::LoadingVideo);
            }
            HostEvent::Failed(PlaybackError::NoPeers) => self.status = Status::NoPeers,
            HostEvent::Failed(error) => self.status = Status::Error(error),
            HostEvent::SourceChanged(source) => {
                self.source = source;
                self.falling_back = false;
            }
            HostEvent::FallingBack => self.falling_back = true,
            HostEvent::Closed => {
                let (title, source, fullscreen) =
                    (self.title.clone(), self.source, self.fullscreen);
                *self = Self::new(title, source);
                self.fullscreen = fullscreen;
            }
            HostEvent::Volume(volume) => self.volume = volume.min(100),
            HostEvent::Muted(muted) => self.muted = muted,
            HostEvent::Seeked(seconds) => {
                self.seek_from = Some(seconds);
                self.time = finite(Some(seconds));
                if self.known_file && self.time.is_some() && self.status == Status::Ended {
                    self.status = Status::Ready;
                }
            }
            HostEvent::Mpv(event) => self.mpv(event),
            HostEvent::Link(_) => {}
            HostEvent::Torrent(health) => self.torrent_health(health),
        }
        self.has_reached_playable |= matches!(
            self.status,
            Status::Ready | Status::Buffering | Status::Ended
        );
    }

    fn mpv(&mut self, event: PlayerEvent) {
        let live = self.known_file
            && !matches!(
                self.status,
                Status::Idle | Status::Error(_) | Status::NoPeers
            );
        match event {
            PlayerEvent::TimePos(value) => {
                let value = finite(value);
                if let (Some(before), Some(now)) = (self.time, value) {
                    if now != before {
                        self.moved = true;
                    }
                    if self.seek_from.is_some_and(|from| now > from + 0.25) {
                        self.seek_from = None;
                    }
                }
                self.time = value;
                if live && self.time.is_some() && matches!(self.status, Status::Preparing(_)) {
                    self.status = Status::Ready;
                }
            }
            PlayerEvent::Duration(value) => {
                self.duration = finite(value).filter(|value| *value > 0.0)
            }
            PlayerEvent::Pause(paused) => self.paused = paused,
            PlayerEvent::EofReached(eof) => {
                if eof && !self.eof_reached && live {
                    self.status = Status::Ended;
                }
                self.eof_reached = eof;
            }
            PlayerEvent::PausedForCache(buffering)
                if live && !matches!(self.status, Status::Ended) =>
            {
                if buffering {
                    // A stall is a cache pause while playing: not before this load has
                    // moved, not while a seek settles, not while the user has paused.
                    if self.has_reached_playable
                        && self.moved
                        && self.seek_from.is_none()
                        && !self.paused
                        && self.status != Status::Buffering
                    {
                        self.rebuffers = self.rebuffers.saturating_add(1);
                    }
                    self.status = Status::Buffering;
                } else if self.has_reached_playable {
                    self.status = Status::Ready;
                }
            }
            PlayerEvent::DemuxerCacheState(cache) => {
                self.buffered_until = finite(cache.cache_end_seconds);
                self.cache_seconds = finite(cache.buffered_seconds);
            }
            PlayerEvent::HwdecCurrent(value) => self.hwdec = value,
            PlayerEvent::SubText(text) => {
                self.cue = text
                    .as_deref()
                    .and_then(super::subtitle_render::sanitize_cue);
            }
            PlayerEvent::AudioDeviceList(devices) => {
                self.devices = devices
                    .into_iter()
                    .map(|device| super::output::Device {
                        name: device.name,
                        label: device.description,
                        passthrough: vec![],
                    })
                    .collect();
            }
            PlayerEvent::TrackList(tracks) => {
                self.audio = tracks
                    .iter()
                    .filter(|track| track.kind == TrackKind::Audio)
                    .enumerate()
                    .map(|(index, track)| track_value(track, index))
                    .collect();
                self.subtitles = tracks
                    .iter()
                    .filter(|track| track.kind == TrackKind::Subtitle)
                    .enumerate()
                    .map(|(index, track)| track_value(track, index))
                    .collect();
            }
            PlayerEvent::EndFile {
                reason: EndReason::Eof,
                ..
            } if live => self.status = Status::Ended,
            // The torrent has already said why nothing plays; mpv's own failure adds nothing.
            PlayerEvent::EndFile {
                reason: EndReason::Error(_),
                ..
            }
            | PlayerEvent::Error(_)
                if self.status == Status::NoPeers => {}
            PlayerEvent::EndFile {
                reason: EndReason::Error(error),
                ..
            }
            | PlayerEvent::Error(error) => self.status = Status::Error(error.into()),
            _ => {}
        }
    }

    pub fn initial_preparing(&self) -> bool {
        !self.has_reached_playable && matches!(self.status, Status::Preparing(_))
    }

    pub fn surface_visible(&self) -> bool {
        matches!(
            self.status,
            Status::Preparing(_) | Status::Ready | Status::Buffering | Status::Ended
        ) || (self.status == Status::NoPeers && self.has_reached_playable)
    }
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value >= 0.0)
}

#[path = "model_torrent.rs"]
mod torrent;

#[path = "model_tracks.rs"]
mod tracks;
use tracks::track_value;

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "model_torrent_tests.rs"]
mod torrent_tests;
