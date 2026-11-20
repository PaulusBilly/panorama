use std::fmt;

use serde::{Deserialize, Serialize};

use super::{MAX_LINE, MAX_MEMBERS, release::PartyRelease};
use crate::addons::{Kind, valid_id};

const MAX_POSITION: f64 = 86_400.0;
const MAX_NAME: usize = 32;
const MAX_TITLE: usize = 200;
const MAX_LABEL: usize = 5000;
const MAX_BINGE_GROUP: usize = 200;

/// Member to host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum Up {
    /// Must be the first message on a connection.
    Hello {
        name: String,
        version: u16,
        secret: String,
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
    },
    Roster {
        members: Vec<MemberView>,
    },
    Notice(Notice),
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
            self.anchor_position + (host_ms - self.anchor_host_ms) as f64 / 1000.0
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
            } => {
                check(is_hex(&secret, 32), "secret")?;
                Self::Hello {
                    name: name_of(&name)?,
                    version,
                    secret,
                }
            }
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
            Self::Ping { .. } | Self::Request { .. } => self,
        })
    }
}

impl Down {
    fn validated(self) -> Result<Self, ProtocolError> {
        match &self {
            Self::Load { media, .. } => media.check()?,
            Self::State { playback, .. } => {
                check(is_position(playback.anchor_position), "position")?
            }
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
            Self::Welcome { .. } | Self::Pong { .. } | Self::Bye { .. } => {}
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
    check(!name.chars().any(char::is_control), "name")?;
    Ok(name.to_owned())
}

fn is_position(value: f64) -> bool {
    value.is_finite() && (0.0..=MAX_POSITION).contains(&value)
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
