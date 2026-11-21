use std::collections::BTreeMap;

use super::{
    MAX_MEMBERS, PROTOCOL_VERSION,
    protocol::{
        Action, ByeReason, Down, MAX_NAME, MAX_POSITION, MemberState, MemberView, Notice,
        NoticeKind, PartyMedia, PlaybackState, TrackChoice, Up,
    },
    transport::PartySecret,
};

const HELLO_TIMEOUT_MS: i64 = 10_000;
const READY_TIMEOUT_MS: i64 = 20_000;
const BUFFER_GRACE_MS: i64 = 1_500;
const WAIT_LIMIT_MS: i64 = 30_000;
const WAITS_PER_MEDIA: u8 = 3;
const REFRESH_MS: i64 = 5_000;
const LENGTH_TOLERANCE: f64 = 2.0;
/// A wait pauses the party where the buffering player stopped when it is at most this far
/// behind the timeline, so it resumes without skipping what it missed.
const CATCH_UP: f64 = 5.0;
/// The host's picture is the party's: once it strays this far from the timeline, the timeline
/// moves to it.
const HOST_DRIFT: f64 = 0.5;
/// How long after the timeline last moved the host's reports are trusted to show the move.
const SETTLE_MS: i64 = 1_500;
/// A tick: however often members' reports move the roster, everyone hears it at most this often.
const ROSTER_EVERY_MS: i64 = 250;
const HOST_ID: u32 = 0;

/// A connection, numbered by the transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ConnId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSettings {
    /// Pause the party while anyone buffers.
    pub wait_for_everyone: bool,
    /// Members' play, pause and seek requests apply; otherwise only the host controls.
    pub anyone_controls: bool,
}

impl Default for HostSettings {
    fn default() -> Self {
        Self {
            wait_for_everyone: true,
            anyone_controls: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HostInput {
    Connected {
        conn: ConnId,
    },
    Message {
        conn: ConnId,
        msg: Up,
    },
    Disconnected {
        conn: ConnId,
    },
    /// The host's own controls.
    Local(Action),
    /// The host's own player.
    LocalReport {
        state: MemberState,
        position: f64,
        duration: Option<f64>,
    },
    /// The host opened `media` at `at` seconds; everyone opens it paused there.
    SetMedia {
        media: PartyMedia,
        at: f64,
    },
    /// Leave the lobby: the party plays once everyone is ready. Nothing to start before media.
    Start,
    /// The host's player changed track, or named its tracks for new media.
    Tracks(TrackChoice),
    Settings(HostSettings),
    End,
    /// About every 250 ms.
    Tick,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HostOutput {
    Send {
        conn: ConnId,
        msg: Down,
    },
    Drop {
        conn: ConnId,
    },
    /// Move the host's own player to the party's timeline.
    Apply(PlaybackState),
    Roster(Vec<MemberView>),
    Notice(Notice),
}

/// The party's authority. Feed it every input with the host's monotonic `now_ms`; it answers
/// with what to send, whom to drop and what the host's own player and panel should show.
#[derive(Debug)]
pub struct HostSession {
    secret: PartySecret,
    settings: HostSettings,
    host: Participant,
    conns: BTreeMap<ConnId, Conn>,
    next_member: u32,
    showing: Option<Showing>,
    tracks: Option<TrackChoice>,
    revision: u64,
    published: Option<Published>,
    published_ms: i64,
    roster: Vec<MemberView>,
    roster_ms: Option<i64>,
    ended: bool,
}

#[derive(Debug)]
enum Conn {
    AwaitingHello { since: i64 },
    Joined(Participant),
}

#[derive(Debug)]
struct Showing {
    media: PartyMedia,
    phase: Phase,
    anchor_position: f64,
    anchor_host_ms: i64,
}

/// What `Down::State` carries besides its revision.
#[derive(Clone, Debug, PartialEq)]
struct Published {
    playback: PlaybackState,
    anyone_controls: bool,
    lobby: bool,
    waiting_for: Option<u32>,
    tracks: Option<TrackChoice>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    /// Before the host starts: everyone opens the media paused and nothing plays.
    Lobby,
    /// Everyone opens the media paused; the party starts once all are ready.
    WaitingForReady {
        since: i64,
    },
    Playing,
    Paused,
    /// Paused because `member` is buffering; resumes when nobody is.
    WaitingFor {
        member: u32,
        since: i64,
    },
}

#[derive(Debug)]
struct Participant {
    id: u32,
    name: String,
    state: MemberState,
    position: f64,
    duration: Option<f64>,
    release_matched: bool,
    late: bool,
    buffering_since: Option<i64>,
    /// When the last report arrived.
    reported_ms: i64,
    waits: u8,
    told_release: bool,
    told_length: bool,
}

impl Participant {
    fn new(id: u32, name: String) -> Self {
        Self {
            id,
            name,
            state: MemberState::Loading,
            position: 0.0,
            duration: None,
            release_matched: true,
            late: false,
            buffering_since: None,
            reported_ms: 0,
            waits: 0,
            told_release: false,
            told_length: false,
        }
    }

    fn reset(&mut self) {
        *self = Self::new(self.id, std::mem::take(&mut self.name));
    }

    fn report(&mut self, now: i64, state: MemberState, position: f64, duration: Option<f64>) {
        if state != MemberState::Buffering {
            self.buffering_since = None;
        } else if self.state != MemberState::Buffering {
            self.buffering_since = Some(now);
        }
        if matches!(
            state,
            MemberState::Ready | MemberState::Playing | MemberState::Paused
        ) {
            self.late = false;
        }
        self.state = state;
        self.position = position;
        self.duration = duration;
        self.reported_ms = now;
    }

    fn holds_start(&self) -> bool {
        matches!(self.state, MemberState::Loading | MemberState::Buffering)
    }

    fn can_hold(&self) -> bool {
        self.waits < WAITS_PER_MEDIA
    }

    fn view(&self, is_host: bool) -> MemberView {
        MemberView {
            member_id: self.id,
            name: self.name.clone(),
            state: self.state,
            late: self.late,
            release_matched: self.release_matched,
            is_host,
        }
    }
}

impl Showing {
    fn playback(&self) -> PlaybackState {
        PlaybackState {
            playing: self.phase == Phase::Playing,
            anchor_position: self.anchor_position,
            anchor_host_ms: self.anchor_host_ms,
        }
    }

    fn waiting_for(&self) -> Option<u32> {
        match self.phase {
            Phase::WaitingFor { member, .. } => Some(member),
            _ => None,
        }
    }

    /// `duration` is the host's length of the media, when its player knows it.
    fn set(&mut self, now: i64, phase: Phase, position: f64, duration: Option<f64>) {
        self.phase = phase;
        self.anchor_position = within(position, duration);
        self.anchor_host_ms = now;
    }
}

impl HostSession {
    /// `secret` is the party secret from the invite.
    pub fn new(name: String, secret: PartySecret, settings: HostSettings) -> Self {
        Self {
            secret,
            settings,
            host: Participant::new(HOST_ID, name),
            conns: BTreeMap::new(),
            next_member: HOST_ID + 1,
            showing: None,
            tracks: None,
            revision: 0,
            published: None,
            published_ms: 0,
            roster: Vec::new(),
            roster_ms: None,
            ended: false,
        }
    }

    pub fn handle(&mut self, now_ms: i64, input: HostInput) -> Vec<HostOutput> {
        let mut out = Vec::new();
        if self.ended {
            return out;
        }
        match input {
            HostInput::Connected { conn } => {
                self.conns
                    .entry(conn)
                    .or_insert(Conn::AwaitingHello { since: now_ms });
            }
            HostInput::Message { conn, msg } => self.message(now_ms, conn, msg, &mut out),
            HostInput::Disconnected { conn } => self.disconnect(conn, &mut out),
            HostInput::Local(action) => {
                let by = self.host.name.clone();
                self.apply(now_ms, action, by, &mut out);
            }
            HostInput::LocalReport {
                state,
                position,
                duration,
            } => self.host.report(now_ms, state, position, duration),
            HostInput::SetMedia { media, at } => self.load(now_ms, media, at, &mut out),
            HostInput::Start => {
                if let Some(showing) = &mut self.showing
                    && showing.phase == Phase::Lobby
                {
                    showing.phase = Phase::WaitingForReady { since: now_ms };
                }
            }
            HostInput::Tracks(tracks) => self.tracks = Some(tracks.bounded()),
            HostInput::Settings(settings) => self.settings = settings,
            HostInput::End => {
                self.end(&mut out);
                return out;
            }
            HostInput::Tick => {}
        }
        self.advance(now_ms, &mut out);
        self.publish(now_ms, &mut out);
        out
    }

    fn message(&mut self, now: i64, conn: ConnId, msg: Up, out: &mut Vec<HostOutput>) {
        let in_lobby = self.in_lobby();
        let joined = match self.conns.get_mut(&conn) {
            None => return,
            Some(Conn::AwaitingHello { .. }) => {
                if let Up::Hello {
                    name,
                    version,
                    secret,
                } = msg
                {
                    self.hello(conn, name, version, &secret, out);
                } else {
                    self.conns.remove(&conn);
                    out.push(HostOutput::Drop { conn });
                }
                return;
            }
            Some(Conn::Joined(member)) => member,
        };
        match msg {
            Up::Hello { .. } => {
                self.disconnect(conn, out);
                out.push(HostOutput::Drop { conn });
            }
            Up::Ping { id, sent_ms } => out.push(HostOutput::Send {
                conn,
                msg: Down::Pong {
                    id,
                    sent_ms,
                    host_ms: now,
                },
            }),
            Up::Report {
                state,
                position,
                duration,
                release_matched,
            } => {
                joined.report(now, state, position, duration);
                joined.release_matched = release_matched;
            }
            Up::Request {
                action: Action::Play | Action::Pause,
            } if in_lobby => self.deny(conn, out),
            Up::Request { action } if self.settings.anyone_controls => {
                let by = joined.name.clone();
                self.apply(now, action, by, out);
            }
            Up::Request { .. } => self.deny(conn, out),
        }
    }

    fn deny(&self, conn: ConnId, out: &mut Vec<HostOutput>) {
        out.push(HostOutput::Send {
            conn,
            msg: Down::Notice(Notice {
                kind: NoticeKind::Denied,
                by: self.host.name.clone(),
                at: None,
            }),
        });
    }

    /// Start needs media, so a party with none has not started.
    fn in_lobby(&self) -> bool {
        self.showing
            .as_ref()
            .is_none_or(|showing| showing.phase == Phase::Lobby)
    }

    fn hello(
        &mut self,
        conn: ConnId,
        name: String,
        version: u16,
        secret: &PartySecret,
        out: &mut Vec<HostOutput>,
    ) {
        let refusal = if *secret != self.secret {
            Some(ByeReason::BadSecret)
        } else if version != PROTOCOL_VERSION {
            Some(ByeReason::VersionMismatch)
        } else if self.joined().count() + 1 >= MAX_MEMBERS {
            Some(ByeReason::Full)
        } else {
            None
        };
        if let Some(reason) = refusal {
            self.conns.remove(&conn);
            out.push(HostOutput::Send {
                conn,
                msg: Down::Bye { reason },
            });
            out.push(HostOutput::Drop { conn });
            return;
        }
        let name = self.unique_name(name);
        let member_id = self.next_member;
        self.next_member += 1;
        self.conns.insert(
            conn,
            Conn::Joined(Participant::new(member_id, name.clone())),
        );
        out.push(HostOutput::Send {
            conn,
            msg: Down::Welcome { member_id },
        });
        if let Some(showing) = &self.showing {
            for msg in [
                Down::Load {
                    media: showing.media.clone(),
                    revision: self.revision,
                },
                self.state(showing),
            ] {
                out.push(HostOutput::Send { conn, msg });
            }
        }
        self.notify(NoticeKind::Joined, name, None, out);
    }

    /// `name`, or when someone in the party already goes by it, whatever the case, `name (n)`.
    fn unique_name(&self, name: String) -> String {
        let taken = |candidate: &str| {
            let candidate = candidate.to_lowercase();
            std::iter::once(&self.host)
                .chain(self.joined())
                .any(|p| p.name.to_lowercase() == candidate)
        };
        if !taken(&name) {
            return name;
        }
        (2..=MAX_MEMBERS + 1)
            .map(|n| {
                let suffix = format!(" ({n})");
                let base = name
                    .chars()
                    .take(MAX_NAME - suffix.len())
                    .collect::<String>();
                format!("{}{suffix}", base.trim_end())
            })
            .find(|candidate| !taken(candidate))
            .unwrap_or(name)
    }

    fn disconnect(&mut self, conn: ConnId, out: &mut Vec<HostOutput>) {
        if let Some(Conn::Joined(member)) = self.conns.remove(&conn) {
            self.notify(NoticeKind::Left, member.name, None, out);
        }
    }

    fn load(&mut self, now: i64, media: PartyMedia, at: f64, out: &mut Vec<HostOutput>) {
        self.host.reset();
        for conn in self.conns.values_mut() {
            if let Conn::Joined(member) = conn {
                member.reset();
            }
        }
        let phase = if self.in_lobby() {
            Phase::Lobby
        } else {
            Phase::WaitingForReady { since: now }
        };
        self.tracks = None;
        self.revision += 1;
        self.published = None;
        self.broadcast(
            Down::Load {
                media: media.clone(),
                revision: self.revision,
            },
            out,
        );
        self.showing = Some(Showing {
            media,
            phase,
            anchor_position: within(at, None),
            anchor_host_ms: now,
        });
    }

    fn apply(&mut self, now: i64, action: Action, by: String, out: &mut Vec<HostOutput>) {
        let Some(showing) = &mut self.showing else {
            return;
        };
        let position = showing.playback().expected_at(now);
        let (phase, position, kind) = match (action, showing.phase) {
            (Action::Play, Phase::Paused) => (Phase::Playing, position, NoticeKind::Played),
            (Action::Play, _) | (Action::Pause, Phase::Paused | Phase::Lobby) => return,
            (Action::Pause, _) => (Phase::Paused, position, NoticeKind::Paused),
            (Action::Seek { to }, phase) => (phase, to, NoticeKind::Seeked),
        };
        showing.set(now, phase, position, self.host.duration);
        let at = showing.anchor_position;
        self.notify(kind, by, Some(at), out);
    }

    /// Moves the party along on time alone: readiness, buffer waits and their limits, and
    /// mismatch notices.
    fn advance(&mut self, now: i64, out: &mut Vec<HostOutput>) {
        let stale = self
            .conns
            .iter()
            .filter(|(_, conn)| {
                matches!(conn, Conn::AwaitingHello { since } if now - since > HELLO_TIMEOUT_MS)
            })
            .map(|(&conn, _)| conn)
            .collect::<Vec<_>>();
        for conn in stale {
            self.conns.remove(&conn);
            out.push(HostOutput::Drop { conn });
        }
        let Some(phase) = self.showing.as_ref().map(|showing| showing.phase) else {
            return;
        };
        let wait = self.settings.wait_for_everyone;
        let mut notices = Vec::new();
        let mut stopped_at = None;
        let next = match phase {
            Phase::WaitingForReady { since } => {
                let timed_out = now - since > READY_TIMEOUT_MS;
                let mut held = false;
                for member in self.participants_mut().filter(|p| p.holds_start()) {
                    held = true;
                    member.late |= timed_out && member.id != HOST_ID;
                }
                (!held || timed_out).then_some(Phase::Playing)
            }
            Phase::Playing if wait => self
                .participants_mut()
                .find(|p| {
                    p.can_hold()
                        && p.buffering_since
                            .is_some_and(|since| now - since > BUFFER_GRACE_MS)
                })
                .map(|holder| {
                    holder.waits += 1;
                    stopped_at = Some(holder.position);
                    notices.push((NoticeKind::WaitingFor, holder.name.clone()));
                    Phase::WaitingFor {
                        member: holder.id,
                        since: now,
                    }
                }),
            Phase::WaitingFor { member, since } => {
                let timed_out = now - since > WAIT_LIMIT_MS;
                let mut held = false;
                for p in self.participants_mut() {
                    if p.id == member && timed_out {
                        p.waits = WAITS_PER_MEDIA;
                    }
                    held |= p.state == MemberState::Buffering && (p.can_hold() || p.id == member);
                }
                (!wait || timed_out || !held).then_some(Phase::Playing)
            }
            Phase::Lobby | Phase::Playing | Phase::Paused => None,
        };
        let host_duration = self.host.duration;
        for conn in self.conns.values_mut() {
            let Conn::Joined(member) = conn else {
                continue;
            };
            if !member.release_matched && !member.told_release {
                member.told_release = true;
                notices.push((NoticeKind::DifferentRelease, member.name.clone()));
            }
            let differs = matches!((member.duration, host_duration),
                (Some(theirs), Some(ours)) if (theirs - ours).abs() > LENGTH_TOLERANCE);
            if differs && !member.told_length {
                member.told_length = true;
                notices.push((NoticeKind::DifferentLength, member.name.clone()));
            }
        }
        if let (Some(next), Some(showing)) = (next, &mut self.showing) {
            let expected = showing.playback().expected_at(now);
            let position = stopped_at
                .filter(|at| (expected - CATCH_UP..=expected).contains(at))
                .unwrap_or(expected);
            showing.set(now, next, position, host_duration);
        }
        if let Some(showing) = &mut self.showing
            && showing.phase == Phase::Playing
            && self.host.state == MemberState::Playing
            && self.host.reported_ms - showing.anchor_host_ms >= SETTLE_MS
        {
            let here = self.host.position + (now - self.host.reported_ms) as f64 / 1000.0;
            if (here - showing.playback().expected_at(now)).abs() > HOST_DRIFT {
                showing.set(now, Phase::Playing, here, host_duration);
            }
        }
        for (kind, by) in notices {
            let at = match (kind, &self.showing) {
                (NoticeKind::WaitingFor, Some(showing)) => Some(showing.anchor_position),
                _ => None,
            };
            self.notify(kind, by, at, out);
        }
    }

    /// Sends the party's state whenever it changes, and every 5 s as a refresh; and the roster
    /// when someone joins or leaves, and when only reports moved it, at most once a tick.
    fn publish(&mut self, now: i64, out: &mut Vec<HostOutput>) {
        if let Some(showing) = &self.showing {
            let current = Published {
                playback: showing.playback(),
                anyone_controls: self.settings.anyone_controls,
                lobby: showing.phase == Phase::Lobby,
                waiting_for: showing.waiting_for(),
                tracks: self.tracks.clone(),
            };
            let changed = self.published.as_ref() != Some(&current);
            if changed {
                self.revision += 1;
                if self.published.as_ref().map(|published| published.playback)
                    != Some(current.playback)
                {
                    out.push(HostOutput::Apply(current.playback));
                }
                self.published = Some(current);
            }
            if changed || now - self.published_ms >= REFRESH_MS {
                self.published_ms = now;
                self.broadcast(self.state(showing), out);
            }
        }
        let roster = std::iter::once(self.host.view(true))
            .chain(self.joined().map(|member| member.view(false)))
            .collect::<Vec<_>>();
        let ids = |roster: &[MemberView]| roster.iter().map(|m| m.member_id).collect::<Vec<_>>();
        let due = ids(&roster) != ids(&self.roster)
            || self
                .roster_ms
                .is_none_or(|at| now.saturating_sub(at) >= ROSTER_EVERY_MS);
        if roster != self.roster && due {
            self.roster = roster.clone();
            self.roster_ms = Some(now);
            self.broadcast(
                Down::Roster {
                    members: roster.clone(),
                },
                out,
            );
            out.push(HostOutput::Roster(roster));
        }
    }

    fn state(&self, showing: &Showing) -> Down {
        Down::State {
            revision: self.revision,
            playback: showing.playback(),
            anyone_controls: self.settings.anyone_controls,
            lobby: showing.phase == Phase::Lobby,
            waiting_for: showing.waiting_for(),
            tracks: self.tracks.clone(),
        }
    }

    fn end(&mut self, out: &mut Vec<HostOutput>) {
        self.ended = true;
        self.broadcast(
            Down::Bye {
                reason: ByeReason::HostLeft,
            },
            out,
        );
        for conn in std::mem::take(&mut self.conns).into_keys() {
            out.push(HostOutput::Drop { conn });
        }
    }

    fn notify(&self, kind: NoticeKind, by: String, at: Option<f64>, out: &mut Vec<HostOutput>) {
        let notice = Notice { kind, by, at };
        self.broadcast(Down::Notice(notice.clone()), out);
        out.push(HostOutput::Notice(notice));
    }

    fn broadcast(&self, msg: Down, out: &mut Vec<HostOutput>) {
        for (&conn, state) in &self.conns {
            if matches!(state, Conn::Joined(_)) {
                out.push(HostOutput::Send {
                    conn,
                    msg: msg.clone(),
                });
            }
        }
    }

    fn joined(&self) -> impl Iterator<Item = &Participant> {
        self.conns.values().filter_map(|conn| match conn {
            Conn::Joined(member) => Some(member),
            Conn::AwaitingHello { .. } => None,
        })
    }

    fn participants_mut(&mut self) -> impl Iterator<Item = &mut Participant> {
        std::iter::once(&mut self.host).chain(self.conns.values_mut().filter_map(
            |conn| match conn {
                Conn::Joined(member) => Some(member),
                Conn::AwaitingHello { .. } => None,
            },
        ))
    }
}

/// A position members accept: on the media, and never past its end.
fn within(position: f64, duration: Option<f64>) -> f64 {
    let end = duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map_or(MAX_POSITION, |duration| duration.min(MAX_POSITION));
    if position.is_finite() {
        position.clamp(0.0, end)
    } else {
        0.0
    }
}
