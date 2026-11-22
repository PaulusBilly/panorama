//! A watch party run from the app: the network side lives on the services runtime
//! ([`service`]), and the GPUI side sees one [`PartyView`] and applies the party's commands to
//! the player.
pub mod follow;
pub mod handoff;
mod service;
mod state;

use crate::playback::{
    host::Command,
    model::{PlaybackModel, Status},
};
use panorama_core::{
    addons::{StreamGroup, StreamSource},
    party::{
        Action, ByeReason, Gate, HostSettings, MemberState, MemberView, Notice, PartyRelease,
        is_hidden_char,
        transport::{PathKind, TransportError},
    },
    store::{Key, Store},
};
use std::{fmt, time::Instant};

const SYNC: &str = "party.sync";

pub use state::{Party, PendingPlay};

/// Notices kept for the panel and the toast, newest last.
pub const NOTICES: usize = 8;
/// Bytes of a display name, as the protocol allows.
const MAX_NAME: usize = 32;

/// Everything the interface shows about the party.
#[derive(Clone, Debug, PartialEq)]
pub struct PartyView {
    pub role: Role,
    pub link: Link,
    /// The host first, then members in join order.
    pub roster: Vec<RosterRow>,
    /// Whether members' play, pause and seek apply; otherwise only the host's do.
    pub anyone_controls: bool,
    pub notices: Vec<NoticeEntry>,
    /// Which of the host's track choices this person follows.
    pub sync: PartySync,
    /// Member: this person's roster id, once the host welcomed them.
    pub me: Option<u32>,
    /// What holds the party's timeline, once the host has said.
    pub gate: Option<Gate>,
}

/// Whether this person's subtitles and audio follow the host's. Kept per install.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PartySync {
    pub subtitles: bool,
    pub audio: bool,
}

impl Default for PartySync {
    fn default() -> Self {
        Self {
            subtitles: true,
            audio: true,
        }
    }
}

impl PartySync {
    pub(crate) fn to_bytes(self) -> [u8; 2] {
        [u8::from(self.subtitles), u8::from(self.audio)]
    }

    pub(crate) fn load(store: Option<&Store>) -> Self {
        (|| {
            let bytes = store?.get(&Key::pref(SYNC).ok()?).ok()??;
            Some(Self::from_bytes(&bytes))
        })()
        .unwrap_or_default()
    }

    pub(crate) fn save(self, store: Option<&Store>) {
        if let (Some(store), Ok(key)) = (store, Key::pref(SYNC)) {
            let _ = store.set(&key, &self.to_bytes());
        }
    }

    /// Anything unreadable gives the default of following the host.
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        match *bytes {
            [subtitles @ (0 | 1), audio @ (0 | 1)] => Self {
                subtitles: subtitles == 1,
                audio: audio == 1,
            },
            _ => Self::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    Host {
        /// Ready once a relay answers.
        invite: Option<InviteLink>,
        settings: HostSettings,
    },
    Member,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    /// Opening the network endpoint, and for a host, reaching a relay for the invite.
    Starting,
    /// Dialing the host and waiting for its welcome.
    Connecting,
    Live,
    Ended(EndReason),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EndReason {
    /// This person left, or ended the party they hosted.
    Left,
    /// The host said goodbye: it ended the party, was full, or refused the link.
    Bye(ByeReason),
    /// The connection to the host dropped.
    Lost,
    Failed(TransportError),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RosterRow {
    pub member: MemberView,
    /// How this person's connection runs: a member's to the host, or on a member's own
    /// screen, the host row shows its own connection to the host.
    pub path: Option<PathKind>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoticeEntry {
    pub notice: Notice,
    pub at: Instant,
}

/// The invite link. Its secret lets anyone join, so it never prints.
#[derive(Clone, PartialEq, Eq)]
pub struct InviteLink(String);

impl InviteLink {
    /// A made-up link for `--party-preview` captures.
    pub fn preview() -> Self {
        Self(format!(
            "{}#preview",
            panorama_core::party::transport::WEB_INVITE
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for InviteLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InviteLink(..)")
    }
}

impl PartyView {
    fn new(role: Role, link: Link) -> Self {
        Self {
            role,
            link,
            roster: Vec::new(),
            anyone_controls: false,
            notices: Vec::new(),
            sync: PartySync::default(),
            me: None,
            gate: None,
        }
    }

    pub fn is_host(&self) -> bool {
        matches!(self.role, Role::Host { .. })
    }

    pub fn ended(&self) -> Option<EndReason> {
        match self.link {
            Link::Ended(reason) => Some(reason),
            _ => None,
        }
    }

    fn notify(&mut self, notice: Notice) {
        self.notices.push(NoticeEntry {
            notice,
            at: Instant::now(),
        });
        let extra = self.notices.len().saturating_sub(NOTICES);
        self.notices.drain(..extra);
    }

    /// The host's name, as members see it.
    fn host_name(&self) -> String {
        self.roster
            .iter()
            .find(|row| row.member.is_host)
            .map_or_else(|| "Host".to_owned(), |row| row.member.name.clone())
    }
}

/// Seconds of media a paused player needs ahead of it before it counts as ready to play.
const READY_CACHE: f64 = 3.0;

/// What the local player tells the party: its state, position and length. `None` while there
/// is nothing loaded to report.
pub fn member_state(model: &PlaybackModel) -> Option<(MemberState, f64, Option<f64>)> {
    let state = match model.status {
        Status::Idle => return None,
        Status::Preparing(_) => MemberState::Loading,
        Status::Buffering => MemberState::Buffering,
        Status::Ready if model.paused && thin_cache(model) => MemberState::Buffering,
        Status::Ready if model.paused => MemberState::Paused,
        // mpv calls a file ready before its first frames play; until the position moves the
        // party treats it as buffering, so the timeline waits for it.
        // So does a seek, still fetching its new spot while mpv calls it ready.
        Status::Ready if !model.advancing() || model.seeking() => MemberState::Buffering,
        Status::Ready => MemberState::Playing,
        Status::Ended => MemberState::Paused,
        Status::NoPeers | Status::Error(_) => MemberState::Failed,
    };
    let position = model.time.filter(|time| time.is_finite()).unwrap_or(0.0);
    Some((state, position.clamp(0.0, 86_400.0), model.duration))
}

/// Paused with too little media ahead to start playing, except near the end, where little
/// remains to cache.
fn thin_cache(model: &PlaybackModel) -> bool {
    let near_end = model
        .time
        .zip(model.duration)
        .is_some_and(|(time, duration)| duration - time <= READY_CACHE);
    // mpv may still describe the cache around the spot a seek left: only what reaches past
    // the current position counts.
    let past_here = model
        .buffered_until
        .zip(model.time)
        .map(|(end, time)| end - time);
    let ahead = match (model.cache_seconds, past_here) {
        (Some(cached), Some(past_here)) => Some(cached.min(past_here)),
        (cached, past_here) => cached.or(past_here),
    };
    ahead.is_some_and(|ahead| ahead < READY_CACHE) && !near_end
}

/// A user's play, pause or seek as the absolute action the party decides on. Anything else is
/// not the party's.
pub fn user_action(command: &Command, model: &PlaybackModel) -> Option<Action> {
    let within = |to: f64| {
        let end = model.duration.filter(|d| d.is_finite() && *d > 0.0);
        to.is_finite()
            .then(|| to.clamp(0.0, end.unwrap_or(86_400.0).min(86_400.0)))
    };
    match *command {
        Command::TogglePause if model.paused => Some(Action::Play),
        Command::TogglePause => Some(Action::Pause),
        Command::SetPaused(true) => Some(Action::Pause),
        Command::SetPaused(false) => Some(Action::Play),
        Command::Seek(to) => within(to).map(|to| Action::Seek { to }),
        Command::SeekRelative(delta) => {
            within(model.time.unwrap_or(0.0) + delta).map(|to| Action::Seek { to })
        }
        _ => None,
    }
}

/// Whether `source` from `group` is the party's release, by the rules `party::find` matches on.
pub fn release_matches(release: &PartyRelease, group: &StreamGroup, source: &StreamSource) -> bool {
    let ours = PartyRelease::of(group, source);
    let same_addon = ours.addon == release.addon;
    (release.torrent.is_some() && ours.torrent == release.torrent)
        || (same_addon && ours.label == release.label)
        || (same_addon && release.binge_group.is_some() && ours.binge_group == release.binge_group)
}

/// Members never roll into the next episode on their own: the host's next `load` moves them.
pub fn up_next_allowed(party: Option<&PartyView>) -> bool {
    party.is_none_or(|view| view.is_host() || view.ended().is_some())
}

/// A display name the protocol accepts: trimmed, no control or format characters, at most 32
/// characters.
pub fn display_name(raw: &str) -> Option<String> {
    let name = raw
        .chars()
        .filter(|c| !is_hidden_char(*c))
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME)
        .collect::<String>()
        .trim_end()
        .to_owned();
    (!name.is_empty()).then_some(name)
}

/// The OS user's name, for a first party.
pub fn default_name() -> String {
    ["USER", "USERNAME"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|name| display_name(&name))
        .unwrap_or_else(|| "Guest".to_owned())
}

/// Party diagnostics, only with `PANORAMA_PARTY_TRACE=1`. Never pass a link, ticket, secret or
/// stream label.
pub(crate) fn trace(message: impl FnOnce() -> String) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("PANORAMA_PARTY_TRACE").is_some_and(|v| v == "1")) {
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis());
        crate::debug::log(format!("party wall_ms={wall} {}", message()));
    }
}

#[cfg(test)]
mod tests;
