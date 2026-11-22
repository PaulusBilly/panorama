use std::{fmt, ops::RangeInclusive};

use serde::{Deserialize, Serialize};

use super::{MAX_LINE, MAX_MEMBERS, release::PartyRelease, transport::PartySecret};
use crate::addons::{Kind, valid_id};

pub(super) const MAX_POSITION: f64 = 86_400.0;
pub(super) const MAX_NAME: usize = 32;
const MAX_TITLE: usize = 200;
const MAX_LABEL: usize = 5000;
const MAX_BINGE_GROUP: usize = 200;
const MAX_LANGUAGE: usize = 16;
const MAX_CODEC: usize = 32;
/// Milliseconds on a monotonic clock that started with the app: far more than any run lasts,
/// and exact as an `f64`.
const MAX_TIME_MS: i64 = 1 << 53;

/// Member to host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum Up {
    /// Must be the first message on a connection.
    Hello {
        name: String,
        version: u16,
        secret: PartySecret,
    },
    Ping {
        id: u32,
        sent_ms: i64,
    },
    Report {
        state: MemberState,
        position: f64,
        duration: Option<f64>,
        release_matched: bool,
    },
    Request {
        action: Action,
    },
}

/// Host to member.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum Down {
    Welcome {
        member_id: u32,
    },
    Pong {
        id: u32,
        sent_ms: i64,
        host_ms: i64,
    },
    Load {
        media: PartyMedia,
        revision: u64,
    },
    State {
        revision: u64,
        playback: PlaybackState,
        anyone_controls: bool,
        gate: Gate,
        /// `None` until the host's player names its tracks for this media.
        tracks: Option<TrackChoice>,
    },
    Roster {
        members: Vec<MemberView>,
    },
    Notice(Notice),
    /// The host left the film: everyone closes it and waits in the lobby for the next one.
    Stop {
        revision: u64,
    },
    Bye {
        reason: ByeReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Play,
    Pause,
    Seek { to: f64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberState {
    Loading,
    Ready,
    Playing,
    Paused,
    Buffering,
    Failed,
}

/// The party's timeline. At host time `T` the expected position is
/// `anchor_position + (T - anchor_host_ms) / 1000` while playing, else `anchor_position`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackState {
    pub playing: bool,
    pub anchor_position: f64,
    pub anchor_host_ms: i64,
}

impl PlaybackState {
    pub fn expected_at(&self, host_ms: i64) -> f64 {
        if self.playing {
            self.anchor_position + host_ms.saturating_sub(self.anchor_host_ms) as f64 / 1000.0
        } else {
            self.anchor_position
        }
    }
}

/// What the party watches. Carries a release identity, never a stream URL: each member finds
/// the release among its own addon results.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartyMedia {
    pub film_id: String,
    pub kind: Kind,
    /// The episode id, or the film id for a movie.
    pub video_id: String,
    pub title: String,
    pub release: PartyRelease,
}

impl fmt::Debug for PartyMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PartyMedia")
            .field("film_id", &self.film_id)
            .field("video_id", &self.video_id)
            .field("torrent", &self.release.torrent.is_some())
            .finish_non_exhaustive()
    }
}

/// What holds the party's timeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// Nothing plays until the host starts the party.
    Lobby,
    Ready,
    /// The party is paused while this member buffers, the host included.
    Waiting(u32),
}

/// The host's audio and subtitle tracks. A `None` track is one the host turned off.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackChoice {
    pub audio: Option<TrackKey>,
    pub subtitle: Option<TrackKey>,
}

/// A track as every member's player can find it. Never an mpv track id: those number the
/// tracks of one loaded file, and addon subtitles arrive in a different order on each machine.
/// Compare a local track with a received key through [`TrackKey::bounded`].
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackKey {
    pub language: Option<String>,
    pub title: Option<String>,
    pub codec: Option<String>,
    /// Loaded from outside the media file, such as an addon subtitle.
    pub external: bool,
}

impl fmt::Debug for TrackKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrackKey")
            .field("language", &self.language)
            .field("codec", &self.codec)
            .field("external", &self.external)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberView {
    pub member_id: u32,
    pub name: String,
    pub state: MemberState,
    /// Not ready when the party started without them.
    pub late: bool,
    pub release_matched: bool,
    pub is_host: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub kind: NoticeKind,
    /// The member the notice is about.
    pub by: String,
    pub at: Option<f64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    Paused,
    Played,
    Seeked,
    Joined,
    Left,
    WaitingFor,
    DifferentRelease,
    DifferentLength,
    Denied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ByeReason {
    Full,
    VersionMismatch,
    BadSecret,
    Kicked,
    HostLeft,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    TooLong,
    Malformed,
    Invalid(&'static str),
}

/// One message as a line, without the trailing newline.
pub fn encode<T: Serialize>(msg: &T) -> String {
    serde_json::to_string(msg).expect("party messages serialize to JSON")
}

pub fn decode_up(line: &str) -> Result<Up, ProtocolError> {
    decode::<Up>(line)?.validated()
}

pub fn decode_down(line: &str) -> Result<Down, ProtocolError> {
    decode::<Down>(line)?.validated()
}

fn decode<T: for<'de> Deserialize<'de>>(line: &str) -> Result<T, ProtocolError> {
    if line.len() > MAX_LINE {
        return Err(ProtocolError::TooLong);
    }
    serde_json::from_str(line).map_err(|_| ProtocolError::Malformed)
}

impl Up {
    fn validated(self) -> Result<Self, ProtocolError> {
        Ok(match self {
            Self::Hello {
                name,
                version,
                secret,
            } => Self::Hello {
                name: name_of(&name)?,
                version,
                secret,
            },
            Self::Report {
                position, duration, ..
            } => {
                check(is_position(position), "position")?;
                check(duration.is_none_or(is_position), "duration")?;
                self
            }
            Self::Request {
                action: Action::Seek { to },
            } => {
                check(is_position(to), "position")?;
                self
            }
            Self::Ping { sent_ms, .. } => {
                check(is_time(sent_ms), "time")?;
                self
            }
            Self::Request { .. } => self,
        })
    }
}

impl Down {
    fn validated(self) -> Result<Self, ProtocolError> {
        match &self {
            Self::Load { media, .. } => media.check()?,
            Self::State {
                playback, tracks, ..
            } => {
                check(is_position(playback.anchor_position), "position")?;
                check(is_time(playback.anchor_host_ms), "time")?;
                if let Some(tracks) = tracks {
                    tracks.check()?;
                }
            }
            Self::Pong {
                sent_ms, host_ms, ..
            } => check(is_time(*sent_ms) && is_time(*host_ms), "time")?,
            Self::Roster { members } => {
                check(members.len() <= MAX_MEMBERS, "roster")?;
                for member in members {
                    check(name_of(&member.name)? == member.name, "name")?;
                }
            }
            Self::Notice(notice) => {
                check(name_of(&notice.by)? == notice.by, "name")?;
                check(notice.at.is_none_or(is_position), "position")?;
            }
            Self::Welcome { .. } | Self::Stop { .. } | Self::Bye { .. } => {}
        }
        Ok(self)
    }
}

impl PartyMedia {
    fn check(&self) -> Result<(), ProtocolError> {
        check(valid_id(&self.film_id), "film_id")?;
        check(valid_id(&self.video_id), "video_id")?;
        check(self.title.chars().count() <= MAX_TITLE, "title")?;
        let release = &self.release;
        check(is_manifest_id(&release.addon), "addon")?;
        check(
            release
                .torrent
                .as_ref()
                .is_none_or(|torrent| is_hex(&torrent.info_hash, 40)),
            "info_hash",
        )?;
        check(release.label.chars().count() <= MAX_LABEL, "label")?;
        check(
            release
                .binge_group
                .as_ref()
                .is_none_or(|group| group.chars().count() <= MAX_BINGE_GROUP),
            "binge_group",
        )
    }
}

impl TrackChoice {
    fn check(&self) -> Result<(), ProtocolError> {
        for key in [&self.audio, &self.subtitle].into_iter().flatten() {
            key.check()?;
        }
        Ok(())
    }

    pub fn bounded(self) -> Self {
        Self {
            audio: self.audio.map(TrackKey::bounded),
            subtitle: self.subtitle.map(TrackKey::bounded),
        }
    }
}

impl TrackKey {
    /// The key as it travels: hidden characters dropped and each field cut to its limit.
    pub fn bounded(self) -> Self {
        let cut = |text: Option<String>, max| {
            text.map(|text| {
                text.chars()
                    .filter(|c| !is_hidden_char(*c))
                    .take(max)
                    .collect()
            })
        };
        Self {
            language: cut(self.language, MAX_LANGUAGE),
            title: cut(self.title, MAX_TITLE),
            codec: cut(self.codec, MAX_CODEC),
            external: self.external,
        }
    }

    fn check(&self) -> Result<(), ProtocolError> {
        let fits = |text: &Option<String>, max| {
            text.as_ref()
                .is_none_or(|text| text.chars().count() <= max && !text.chars().any(is_hidden_char))
        };
        check(fits(&self.language, MAX_LANGUAGE), "language")?;
        check(fits(&self.title, MAX_TITLE), "track_title")?;
        check(fits(&self.codec, MAX_CODEC), "codec")
    }
}

fn check(ok: bool, field: &'static str) -> Result<(), ProtocolError> {
    if ok {
        Ok(())
    } else {
        Err(ProtocolError::Invalid(field))
    }
}

fn name_of(name: &str) -> Result<String, ProtocolError> {
    let name = name.trim();
    check((1..=MAX_NAME).contains(&name.chars().count()), "name")?;
    check(!name.chars().any(is_hidden_char), "name")?;
    Ok(name.to_owned())
}

/// Unicode's format characters (category Cf): they draw nothing, yet reorder or hide the text
/// around them, so a name could pose as another.
const FORMAT: [RangeInclusive<char>; 21] = [
    '\u{00AD}'..='\u{00AD}',
    '\u{0600}'..='\u{0605}',
    '\u{061C}'..='\u{061C}',
    '\u{06DD}'..='\u{06DD}',
    '\u{070F}'..='\u{070F}',
    '\u{0890}'..='\u{0891}',
    '\u{08E2}'..='\u{08E2}',
    '\u{180E}'..='\u{180E}',
    '\u{200B}'..='\u{200F}',
    '\u{202A}'..='\u{202E}',
    '\u{2060}'..='\u{2064}',
    '\u{2066}'..='\u{206F}',
    '\u{FEFF}'..='\u{FEFF}',
    '\u{FFF9}'..='\u{FFFB}',
    '\u{110BD}'..='\u{110BD}',
    '\u{110CD}'..='\u{110CD}',
    '\u{13430}'..='\u{1343F}',
    '\u{1BCA0}'..='\u{1BCA3}',
    '\u{1D173}'..='\u{1D17A}',
    '\u{E0001}'..='\u{E0001}',
    '\u{E0020}'..='\u{E007F}',
];

/// Whether a display name may not hold `c`: a control or format character.
pub fn is_hidden_char(c: char) -> bool {
    c.is_control() || FORMAT.iter().any(|range| range.contains(&c))
}

fn is_position(value: f64) -> bool {
    value.is_finite() && (0.0..=MAX_POSITION).contains(&value)
}

fn is_time(ms: i64) -> bool {
    (0..=MAX_TIME_MS).contains(&ms)
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The rule `AddonClient` applies to a manifest id before it builds an `AddonKey` from it.
fn is_manifest_id(id: &str) -> bool {
    (1..=450).contains(&id.len())
        && !id.chars().any(|c| {
            c.is_control() || c.is_whitespace() || matches!(c, '/' | ':' | '|' | '#' | '?' | '\\')
        })
}
