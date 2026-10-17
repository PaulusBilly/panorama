use crate::MpvError;

/// A retained video-surface lease. Obtain it from `VideoSurface::raw_window`.
/// The parent must stay alive and pump messages until shutdown completes.
#[derive(Clone)]
pub struct RawWindow {
    #[cfg(windows)]
    pub(crate) lease: std::sync::Arc<crate::win32::WindowLease>,
}

impl RawWindow {
    pub(crate) fn id(&self) -> usize {
        #[cfg(windows)]
        {
            self.lease.id
        }
        #[cfg(not(windows))]
        {
            0
        }
    }
}

/// Initialization options; overrides are applied after the Electron defaults.
#[derive(Default)]
pub struct PlayerOptions {
    /// Owned child-window lease; `None` is suitable for null-output tests.
    pub wid: Option<RawWindow>,
    /// Explicit option overrides, in order. Values never appear in diagnostics.
    pub extra: Vec<(String, String)>,
}

/// Seek interpretation; both variants request exact seeking.
#[derive(Clone, Copy, Debug)]
pub enum SeekMode {
    /// Seconds from the beginning of the media.
    Absolute,
    /// Signed offset from the current position.
    Relative,
}

/// Copied scalar accepted by the property API.
pub enum PropertyValue {
    /// mpv flag.
    Bool(bool),
    /// Signed 64-bit integer.
    Integer(i64),
    /// Finite double.
    Double(f64),
    /// UTF-8 string without NUL.
    String(String),
}

/// Converts a Rust scalar into an owned mpv property value.
pub trait MpvValue {
    /// Performs conversion and validation without touching libmpv.
    fn into_mpv_value(self) -> Result<PropertyValue, MpvError>;
}

impl MpvValue for bool {
    fn into_mpv_value(self) -> Result<PropertyValue, MpvError> {
        Ok(PropertyValue::Bool(self))
    }
}
impl MpvValue for i64 {
    fn into_mpv_value(self) -> Result<PropertyValue, MpvError> {
        Ok(PropertyValue::Integer(self))
    }
}
impl MpvValue for f64 {
    fn into_mpv_value(self) -> Result<PropertyValue, MpvError> {
        if !self.is_finite() {
            return Err(MpvError::InvalidArgument);
        }
        Ok(PropertyValue::Double(self))
    }
}
impl MpvValue for String {
    fn into_mpv_value(self) -> Result<PropertyValue, MpvError> {
        if self.contains('\0') {
            return Err(MpvError::InvalidArgument);
        }
        Ok(PropertyValue::String(self))
    }
}
impl MpvValue for &str {
    fn into_mpv_value(self) -> Result<PropertyValue, MpvError> {
        self.to_owned().into_mpv_value()
    }
}

/// Track category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackKind {
    /// Audio stream.
    Audio,
    /// Video stream.
    Video,
    /// Subtitle stream.
    Subtitle,
}

/// Owned track-list entry; IDs are local to the loaded media.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// mpv track identifier.
    pub id: i64,
    /// Stream category.
    pub kind: TrackKind,
    /// Optional descriptive title.
    pub title: Option<String>,
    /// Optional language code.
    pub lang: Option<String>,
    /// Optional codec name.
    pub codec: Option<String>,
    /// Default stream flag.
    pub default: bool,
    /// Forced stream flag.
    pub forced: bool,
    /// Loaded from an external source.
    pub external: bool,
    /// Accessibility subtitle flag.
    pub hearing_impaired: bool,
    /// Currently selected stream.
    pub selected: bool,
}

/// Owned audio output device entry.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioDevice {
    /// Value accepted by the `audio-device` property.
    pub name: String,
    /// User-facing description from mpv.
    pub description: String,
}

/// Player measurements for the proxy's `BufferingSample`.
/// Transport bitrate, demand, throttling, and monotonic time come from PR 3.2.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CacheState {
    /// Forward buffered media duration (`demuxer-cache-duration`).
    pub buffered_seconds: Option<f64>,
    /// Last buffered media timestamp (`demuxer-cache-time`).
    pub cache_end_seconds: Option<f64>,
    /// Forward bytes (`demuxer-cache-state/fw-bytes`).
    pub forward_bytes: Option<i64>,
    /// Download bytes per second (`cache-speed`); multiply by 8 / 1e6 for Mbps.
    pub input_bytes_per_second: Option<f64>,
    /// Player is waiting for cache.
    pub buffering: bool,
    /// User pause state.
    pub paused: bool,
    /// Current playback speed; defaults to 1.
    pub playback_speed: f64,
}

/// Sanitized end-file reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndReason {
    /// Natural end of media.
    Eof,
    /// Explicit stop or replacement.
    Stop,
    /// Player quit.
    Quit,
    /// Loading or playback failed; Display is the sanitized message.
    Error(MpvError),
    /// Playlist redirection.
    Redirect,
    /// A future runtime reason.
    Other,
}

/// Copied events consumed by one GPUI background task.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerEvent {
    /// Initialization and property subscriptions succeeded.
    Ready,
    /// Current seconds, or unavailable after stop.
    TimePos(Option<f64>),
    /// Media duration, if known.
    Duration(Option<f64>),
    /// User pause changed.
    Pause(bool),
    /// Cache pause changed.
    PausedForCache(bool),
    /// Buffering completion percent, if known.
    CacheBufferingState(Option<f64>),
    /// Updated measurements for buffering policy.
    DemuxerCacheState(CacheState),
    /// Active hardware decoder, if any.
    HwdecCurrent(Option<String>),
    /// Copied and parsed tracks.
    TrackList(Vec<Track>),
    /// Copied output device list.
    AudioDeviceList(Vec<AudioDevice>),
    /// Subtitle text for a renderer bridge; mpv keeps rendering by default.
    SubText(Option<String>),
    /// Full subtitle start time, including delayed cues.
    SubStart(Option<f64>),
    /// Full subtitle end time, including delayed cues.
    SubEnd(Option<f64>),
    /// Media ended, with playlist entry ID to distinguish replacements.
    EndFile {
        /// Sanitized reason and message.
        reason: EndReason,
        /// mpv playlist entry ID.
        playlist_entry_id: i64,
    },
    /// Sanitized startup or asynchronous operation failure.
    Error(MpvError),
    /// mpv is destroyed; the retained child lease has been released.
    /// On timeout this arrives later, after actual destruction.
    Shutdown,
}
