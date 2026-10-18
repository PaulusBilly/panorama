pub use super::selection::SourceRef;
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
    pub source: SourceRef,
    pub hwdec: Option<String>,
    pub fullscreen: bool,
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
    Closed,
    Volume(u8),
    Muted(bool),
    Seeked(f64),
    Link(super::health::LinkHealth),
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
            Self::Closed => f.write_str("Closed"),
            Self::Volume(volume) => f.debug_tuple("Volume").field(volume).finish(),
            Self::Muted(muted) => f.debug_tuple("Muted").field(muted).finish(),
            Self::Seeked(seconds) => f.debug_tuple("Seeked").field(seconds).finish(),
            Self::Link(_) => f.write_str("Link"),
        }
    }
}

impl PlaybackModel {
    pub fn new(title: String, source: SourceRef) -> Self {
        Self {
            status: Status::Idle,
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
            source,
            hwdec: None,
            fullscreen: false,
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
                self.hwdec = None;
            }
            HostEvent::Loading => {
                self.known_file = true;
                self.status = Status::Preparing(Stage::LoadingVideo);
            }
            HostEvent::Failed(error) => self.status = Status::Error(error),
            HostEvent::SourceChanged(source) => self.source = source,
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
        }
        self.has_reached_playable |= matches!(
            self.status,
            Status::Ready | Status::Buffering | Status::Ended
        );
    }

    fn mpv(&mut self, event: PlayerEvent) {
        let live = self.known_file && !matches!(self.status, Status::Idle | Status::Error(_));
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
        )
    }
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value >= 0.0)
}

fn track_value(track: &panorama_mpv::Track, index: usize) -> Track {
    let label = track
        .title
        .as_ref()
        .filter(|title| {
            !title.trim().is_empty()
                && !title.trim().starts_with("http:")
                && !title.trim().starts_with("https:")
        })
        .cloned()
        .or_else(|| {
            track
                .lang
                .as_deref()
                .filter(|code| !code.trim().is_empty())
                .map(|code| language(code).unwrap_or(code).to_owned())
        })
        .unwrap_or_else(|| format!("Track {}", index + 1));
    Track {
        id: track.id,
        label,
        description: track.codec.clone(),
        lang: track.lang.clone(),
        codec: track.codec.clone(),
        selected: track.selected,
    }
}

fn language(code: &str) -> Option<&'static str> {
    match code {
        "en" | "eng" => Some("English"),
        "fr" | "fra" | "fre" => Some("French"),
        "de" | "deu" | "ger" => Some("German"),
        "es" | "spa" => Some("Spanish"),
        "it" | "ita" => Some("Italian"),
        "ja" | "jpn" => Some("Japanese"),
        "ko" | "kor" => Some("Korean"),
        "zh" | "zho" | "chi" => Some("Chinese"),
        "pt" | "por" => Some("Portuguese"),
        "ru" | "rus" => Some("Russian"),
        "th" | "tha" => Some("Thai"),
        "ar" | "ara" => Some("Arabic"),
        "hi" | "hin" => Some("Hindi"),
        "nl" | "nld" | "dut" => Some("Dutch"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
