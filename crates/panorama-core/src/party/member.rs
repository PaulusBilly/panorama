use super::{
    PROTOCOL_VERSION,
    clock::ClockSync,
    drift::{Correction, correct},
    protocol::{
        Action, ByeReason, Down, MemberState, MemberView, Notice, PartyMedia, PlaybackState,
        TrackChoice, Up,
    },
    transport::PartySecret,
};

const FAST_PING_MS: i64 = 2_000;
const SLOW_PING_MS: i64 = 10_000;
/// Pings run fast for this long after joining or after the clock went stale.
const FAST_PING_WINDOW_MS: i64 = 10_000;
const REPORT_EVERY_MS: i64 = 1_000;
/// A new timeline this close to the one already playing continues it without a seek.
const CONTINUOUS: f64 = 0.25;
/// Each load sets off a details fetch and a stream lookup across every addon.
const LOAD_EVERY_MS: i64 = 2_000;

#[derive(Clone, Debug, PartialEq)]
pub enum MemberInput {
    /// Send `hello`; the connection to the host is open.
    Start,
    Down(Down),
    /// The local player's latest state and position.
    Player {
        state: MemberState,
        position: f64,
        duration: Option<f64>,
    },
    /// Whether this member found the host's release among its own streams.
    Release {
        matched: bool,
    },
    /// The member's own controls. They ask the host; the player moves when the host answers.
    Local(Action),
    /// The player opened a new file and lost whatever it was told before; the party's
    /// timeline applies again.
    Reopened,
    Leave,
    /// About every 250 ms.
    Tick,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MemberOutput {
    Send(Up),
    Welcomed {
        member_id: u32,
    },
    /// Open this media paused; the party's state follows.
    Load(PartyMedia),
    SetPaused(bool),
    Seek(f64),
    SetSpeed(f64),
    /// Whether this member's controls do anything.
    Controls {
        anyone_controls: bool,
    },
    /// Whether the party waits in its lobby, when that changes.
    Lobby(bool),
    /// The host's tracks for the current media, when they change.
    Tracks(TrackChoice),
    /// The member whose buffering holds the party changed; `None` once nobody does.
    WaitingFor(Option<u32>),
    Roster(Vec<MemberView>),
    Notice(Notice),
    /// The host ended the connection. The player keeps playing.
    Ended(ByeReason),
}

/// One member's side of a party. Feed it every input with the member's monotonic `now_ms`.
#[derive(Debug)]
pub struct MemberSession {
    name: String,
    secret: PartySecret,
    link: Link,
    clock: ClockSync,
    pings: Pings,
    revision: Option<u64>,
    media: Option<PartyMedia>,
    opener: Opener,
    playback: Option<PlaybackState>,
    applied: Option<PlaybackState>,
    anyone_controls: Option<bool>,
    lobby: Option<bool>,
    tracks: Option<TrackChoice>,
    waiting_for: Option<u32>,
    speed: f64,
    last_seek_ms: Option<i64>,
    player: Option<Player>,
    release_matched: bool,
    reported: Option<Reported>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Link {
    Idle,
    Joining,
    Joined,
    Ended,
}

#[derive(Debug, Default)]
struct Pings {
    next_id: u32,
    window_from: i64,
    last_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug)]
struct Player {
    state: MemberState,
    position: f64,
    duration: Option<f64>,
}

/// Hands on the host's media at most once every [`LOAD_EVERY_MS`], the latest winning.
#[derive(Debug, Default)]
struct Opener {
    opened: Option<(PartyMedia, i64)>,
    pending: Option<PartyMedia>,
}

impl Opener {
    fn offer(&mut self, media: PartyMedia) {
        self.pending = Some(media);
    }

    fn due(&mut self, now: i64) -> Option<PartyMedia> {
        let media = self.pending.take()?;
        match &self.opened {
            Some((opened, _)) if *opened == media => None,
            Some((_, at)) if now.saturating_sub(*at) < LOAD_EVERY_MS => {
                self.pending = Some(media);
                None
            }
            _ => {
                self.opened = Some((media.clone(), now));
                Some(media)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Reported {
    state: MemberState,
    release_matched: bool,
    at_ms: i64,
}

impl MemberSession {
    /// `secret` comes from the invite.
    pub fn new(name: String, secret: PartySecret) -> Self {
        Self {
            name,
            secret,
            link: Link::Idle,
            clock: ClockSync::new(),
            pings: Pings::default(),
            revision: None,
            media: None,
            opener: Opener::default(),
            playback: None,
            applied: None,
            anyone_controls: None,
            lobby: None,
            tracks: None,
            waiting_for: None,
            speed: 1.0,
            last_seek_ms: None,
            player: None,
            release_matched: true,
            reported: None,
        }
    }

    pub fn handle(&mut self, now_ms: i64, input: MemberInput) -> Vec<MemberOutput> {
        let mut out = Vec::new();
        if self.link == Link::Ended {
            return out;
        }
        if self.clock.note_local_time(now_ms) {
            self.pings.window_from = now_ms;
            self.pings.last_ms = None;
        }
        let mut moved = false;
        match input {
            MemberInput::Start if self.link == Link::Idle => {
                self.link = Link::Joining;
                self.pings.window_from = now_ms;
                out.push(MemberOutput::Send(Up::Hello {
                    name: self.name.clone(),
                    version: PROTOCOL_VERSION,
                    secret: self.secret.clone(),
                }));
            }
            MemberInput::Start => {}
            MemberInput::Down(msg) => self.down(now_ms, msg, &mut out),
            MemberInput::Player {
                state,
                position,
                duration,
            } => {
                self.player = Some(Player {
                    state,
                    position,
                    duration,
                });
                moved = true;
            }
            MemberInput::Release { matched } => self.release_matched = matched,
            MemberInput::Reopened => {
                self.applied = None;
                self.tracks = None;
                // A new file starts at normal speed.
                self.speed = 1.0;
            }
            MemberInput::Local(action) => {
                if self.link == Link::Joined {
                    out.push(MemberOutput::Send(Up::Request { action }));
                }
            }
            MemberInput::Leave => {
                self.end(&mut out);
                return out;
            }
            MemberInput::Tick => {}
        }
        if self.link == Link::Ended {
            return out;
        }
        if let Some(media) = self.opener.due(now_ms) {
            out.push(MemberOutput::Load(media));
        }
        self.follow(now_ms, &mut out);
        if moved {
            self.correct(now_ms, &mut out);
        }
        self.keep_alive(now_ms, &mut out);
        out
    }

    fn down(&mut self, now: i64, msg: Down, out: &mut Vec<MemberOutput>) {
        match msg {
            Down::Welcome { member_id } => {
                if self.link == Link::Joining {
                    self.link = Link::Joined;
                    out.push(MemberOutput::Welcomed { member_id });
                }
            }
            Down::Pong {
                sent_ms, host_ms, ..
            } => self.clock.record(sent_ms, host_ms, now),
            Down::Load { media, revision } => {
                let repeat = self.revision == Some(revision) && self.media.as_ref() == Some(&media);
                if self.stale(revision) || repeat {
                    return;
                }
                self.revision = Some(revision);
                self.media = Some(media.clone());
                self.playback = None;
                self.applied = None;
                self.tracks = None;
                // Opening a new source resets the player's speed.
                self.speed = 1.0;
                self.last_seek_ms = None;
                self.opener.offer(media);
            }
            Down::State {
                revision,
                playback,
                anyone_controls,
                lobby,
                waiting_for,
                tracks,
            } => {
                if self.stale(revision) {
                    return;
                }
                self.revision = Some(revision);
                self.playback = Some(playback);
                if self.anyone_controls != Some(anyone_controls) {
                    self.anyone_controls = Some(anyone_controls);
                    out.push(MemberOutput::Controls { anyone_controls });
                }
                if self.lobby != Some(lobby) {
                    self.lobby = Some(lobby);
                    out.push(MemberOutput::Lobby(lobby));
                }
                if let Some(tracks) = tracks
                    && self.tracks.as_ref() != Some(&tracks)
                {
                    self.tracks = Some(tracks.clone());
                    out.push(MemberOutput::Tracks(tracks));
                }
                if self.waiting_for != waiting_for {
                    self.waiting_for = waiting_for;
                    out.push(MemberOutput::WaitingFor(waiting_for));
                }
            }
            Down::Roster { members } => out.push(MemberOutput::Roster(members)),
            Down::Notice(notice) => out.push(MemberOutput::Notice(notice)),
            Down::Bye { reason } => {
                self.end(out);
                out.push(MemberOutput::Ended(reason));
            }
        }
    }

    fn stale(&self, revision: u64) -> bool {
        self.revision.is_some_and(|held| revision < held)
    }

    /// Moves the player onto a timeline it has not followed yet. A playing timeline needs the
    /// host's clock to place, so it waits for the first pong.
    fn follow(&mut self, now: i64, out: &mut Vec<MemberOutput>) {
        let Some(playback) = self.playback else {
            return;
        };
        if self.applied == Some(playback) {
            return;
        }
        let host_now = match self.clock.host_now(now) {
            Some(host_now) => host_now,
            None if playback.playing => return,
            None => playback.anchor_host_ms,
        };
        let continues = self.applied.is_some_and(|applied| {
            (applied.expected_at(playback.anchor_host_ms) - playback.anchor_position).abs()
                < CONTINUOUS
        });
        if !continues {
            out.push(MemberOutput::Seek(playback.expected_at(host_now)));
            self.last_seek_ms = Some(now);
        }
        out.push(MemberOutput::SetPaused(!playback.playing));
        self.applied = Some(playback);
    }

    fn correct(&mut self, now: i64, out: &mut Vec<MemberOutput>) {
        let (Some(player), Some(playback)) = (self.player, self.applied) else {
            return;
        };
        let host_now = self.clock.host_now(now);
        let correction = match host_now {
            Some(host_now) if playback.playing && player.state == MemberState::Playing => correct(
                player.position,
                playback.expected_at(host_now),
                self.speed,
                self.last_seek_ms.map_or(i64::MAX, |at| now - at),
            ),
            _ if self.speed != 1.0 => Correction::Speed(1.0),
            _ => Correction::None,
        };
        match correction {
            Correction::None => {}
            Correction::Speed(speed) => self.set_speed(speed, out),
            Correction::Seek(to) => {
                out.push(MemberOutput::Seek(to));
                self.last_seek_ms = Some(now);
                self.set_speed(1.0, out);
            }
        }
    }

    fn keep_alive(&mut self, now: i64, out: &mut Vec<MemberOutput>) {
        if self.link == Link::Idle {
            return;
        }
        let every = if now - self.pings.window_from < FAST_PING_WINDOW_MS {
            FAST_PING_MS
        } else {
            SLOW_PING_MS
        };
        if self.pings.last_ms.is_none_or(|last| now - last >= every) {
            out.push(MemberOutput::Send(Up::Ping {
                id: self.pings.next_id,
                sent_ms: now,
            }));
            self.pings.next_id = self.pings.next_id.wrapping_add(1);
            self.pings.last_ms = Some(now);
        }
        let (Link::Joined, Some(player)) = (self.link, self.player) else {
            return;
        };
        let due = match self.reported {
            None => true,
            Some(last) => {
                last.state != player.state
                    || last.release_matched != self.release_matched
                    || (player.state == MemberState::Playing && now - last.at_ms >= REPORT_EVERY_MS)
            }
        };
        if due {
            out.push(MemberOutput::Send(Up::Report {
                state: player.state,
                position: player.position,
                duration: player.duration,
                release_matched: self.release_matched,
            }));
            self.reported = Some(Reported {
                state: player.state,
                release_matched: self.release_matched,
                at_ms: now,
            });
        }
    }

    fn end(&mut self, out: &mut Vec<MemberOutput>) {
        self.link = Link::Ended;
        if self.waiting_for.take().is_some() {
            out.push(MemberOutput::WaitingFor(None));
        }
        self.set_speed(1.0, out);
    }

    fn set_speed(&mut self, speed: f64, out: &mut Vec<MemberOutput>) {
        if self.speed != speed {
            self.speed = speed;
            out.push(MemberOutput::SetSpeed(speed));
        }
    }
}
