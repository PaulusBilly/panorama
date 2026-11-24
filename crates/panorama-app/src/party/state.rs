//! The party on `AppState`: start, join, leave, settings, and what the player feeds in.
use super::{
    EndReason, PartySync, PartyView, Role, default_name, display_name,
    service::{self, Event, Input},
};
use crate::{
    app_state::{Account, AppState},
    playback::{host::Command, launch::Pick},
};
use futures::StreamExt;
use gpui::{Context, Task};
use panorama_core::{
    addons::{FilmDetails, Kind, ResourceEvent},
    party::{
        Action, Gate, HostSettings, MemberState, PartyMedia, TrackChoice,
        transport::{InviteError, parse_invite},
    },
    store::Key,
};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

const NAME: &str = "party.name";

/// The party this window is in, from start until the user leaves. An ended party stays, so
/// its reason shows, until left.
pub struct Party {
    pub view: PartyView,
    inputs: UnboundedSender<Input>,
    /// Host: the media last announced. Member: the media last loaded.
    media: Option<PartyMedia>,
    /// Member: the host's latest track choice, once it has named its tracks.
    host_tracks: Option<TrackChoice>,
    /// Member: how many times the host switched the source of what plays, for the player to
    /// say so.
    pub switches: u32,
    /// Member: the details of the host's film could not be fetched here.
    lookup_failed: bool,
    /// Member: this player's source failed, so the film can't play here.
    source_failed: bool,
    /// Member: the link and name joined with, for rejoining after a dropped connection.
    invite: Option<String>,
    joined_as: Option<String>,
    /// Member: the party has heard that this person can't play.
    told_blocked: bool,
    /// Member: joins made on their own since the link was last live.
    rejoins: u8,
    _rejoin: Option<Task<()>>,
    _events: Task<()>,
}

/// How long a friend waits before each automatic rejoin. After the last, Rejoin is theirs.
const REJOIN_AFTER: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
];

/// Why a friend can't play the party's film, each with its own way out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Blocked {
    SignedOut,
    NoAddons,
    FilmNotFound,
    SourceFailed,
}

impl Blocked {
    pub fn text(self) -> &'static str {
        match self {
            Self::SignedOut => "Sign in to watch with the party",
            Self::NoAddons => "Add a streaming addon in Stremio, then rejoin",
            Self::FilmNotFound => "Couldn't find this film in your addons",
            Self::SourceFailed => "Your copy of this film won't play",
        }
    }

    fn of(account: &Account, known: bool, no_addons: bool, lookup_failed: bool) -> Option<Self> {
        if lookup_failed {
            return Some(Self::FilmNotFound);
        }
        match account {
            _ if !known => None,
            Account::Authenticating => None,
            Account::SignedIn(_) => no_addons.then_some(Self::NoAddons),
            Account::SignedOut | Account::Error(_) => Some(Self::SignedOut),
        }
    }
}

impl Party {
    /// A party with a fixed view and no network, for `--party-preview` captures.
    pub fn preview(view: PartyView) -> Self {
        let (inputs, _) = tokio::sync::mpsc::unbounded_channel();
        Self {
            view,
            inputs,
            media: None,
            host_tracks: None,
            switches: 0,
            lookup_failed: false,
            source_failed: false,
            invite: None,
            joined_as: None,
            told_blocked: false,
            rejoins: 0,
            _rejoin: None,
            _events: Task::ready(()),
        }
    }

    pub fn media(&self) -> Option<&PartyMedia> {
        self.media.as_ref()
    }

    pub fn host_tracks(&self) -> Option<&TrackChoice> {
        self.host_tracks.as_ref()
    }

    /// Whether the user's play, pause and seek go through the party instead of the player.
    pub fn steers_playback(&self) -> bool {
        self.view.ended().is_none() && (!self.view.is_host() || self.media.is_some())
    }

    /// Whether opening this video would take a friend off what the party is watching.
    pub fn pulls_friend_away(&self, film_id: &str, video_id: &str) -> bool {
        self.view.ended().is_none()
            && !self.view.is_host()
            && self
                .media
                .as_ref()
                .is_none_or(|media| media.film_id != film_id || media.video_id != video_id)
    }

    /// The host's Play reaches everyone once friends are in.
    pub fn plays_for_everyone(&self) -> bool {
        self.view.is_host()
            && self.view.ended().is_none()
            && self.view.roster.iter().any(|row| !row.member.is_host)
    }

    /// What the party pill says, for this person outside the player.
    pub fn pill_label(&self) -> String {
        match self.view.roster.len() {
            0 | 1 => "Watch party".to_owned(),
            count => format!("Watch party · {count}"),
        }
    }

    /// The link to rejoin with, once the connection to the host dropped. A party that ended
    /// on purpose has nothing to rejoin.
    pub fn rejoin_invite(&self) -> Option<&str> {
        matches!(
            self.view.ended(),
            Some(EndReason::Lost | EndReason::Failed(_))
        )
        .then_some(self.invite.as_deref())
        .flatten()
    }

    /// When to join again on its own after the connection dropped, while tries remain.
    fn rejoin_after(&self) -> Option<Duration> {
        self.rejoin_invite()?;
        REJOIN_AFTER.get(usize::from(self.rejoins)).copied()
    }

    fn send(&self, input: Input) {
        let _ = self.inputs.send(input);
    }

    /// Announces `media` unless the party already has it.
    fn announce(&mut self, media: PartyMedia, at: f64) {
        if self.media.as_ref() != Some(&media) {
            self.media = Some(media.clone());
            self.send(Input::Media { media, at });
        }
    }

    /// The host's player opened `media` with no lobby to press Start in, so the party starts
    /// with it. Without this, friends wait in the lobby for a Start nobody can press.
    fn announce_playing(&mut self, media: PartyMedia, at: f64) {
        self.announce(media, at);
        self.send(Input::Start);
    }
}

/// A film to open by id once the account and its addons are ready: for a member, the host's
/// media; for debugging, `--party-play`.
pub struct PendingPlay {
    kind: Kind,
    film_id: String,
    video_id: String,
    pick: Pick,
    stage: Stage,
}

enum Stage {
    Waiting,
    Fetching { _task: Task<()> },
    Ready(Box<FilmDetails>),
}

impl AppState {
    /// The name this install shows in a party.
    pub fn party_name(&self) -> String {
        let stored = (|| {
            let key = Key::pref(NAME).ok()?;
            let bytes = self.services.as_ref()?.store.as_ref()?.get(&key).ok()??;
            display_name(&String::from_utf8(bytes).ok()?)
        })();
        stored.unwrap_or_else(default_name)
    }

    /// Keep a display name for later parties. `false` when it is empty once trimmed.
    pub fn set_party_name(&mut self, name: &str) -> bool {
        let Some(name) = display_name(name) else {
            return false;
        };
        if let (Some(store), Ok(key)) = (
            self.services.as_ref().and_then(|s| s.store.as_ref()),
            Key::pref(NAME),
        ) {
            let _ = store.set(&key, name.as_bytes());
        }
        true
    }

    /// Whether this install's subtitles and audio follow the host's.
    pub fn party_sync(&self) -> PartySync {
        PartySync::load(self.services.as_ref().and_then(|s| s.store.as_deref()))
    }

    /// Keep which of the host's tracks this install follows, now and in later parties.
    pub fn set_party_sync(&mut self, sync: PartySync, cx: &mut Context<Self>) {
        sync.save(self.services.as_ref().and_then(|s| s.store.as_deref()));
        if let Some(party) = self.party.as_mut() {
            party.view.sync = sync;
            party.send(Input::Sync(sync));
        }
        cx.notify();
    }

    /// Host: the local player's audio and subtitle tracks, after the media was announced.
    pub fn party_tracks(&self, tracks: TrackChoice) {
        if let Some(party) = self.party.as_ref().filter(|party| party.view.is_host()) {
            party.send(Input::Tracks(tracks));
        }
    }

    /// Host a party. Whatever plays next, or plays now, is what the party watches. A party this
    /// window already hosts is kept, so its friends stay; a party it joined is left first.
    pub fn start_party(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self
            .party
            .as_ref()
            .is_some_and(|party| party.view.is_host() && party.view.ended().is_none())
        {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        let name = name
            .and_then(|name| display_name(&name))
            .unwrap_or_else(|| self.party_name());
        let settings = HostSettings::default();
        let sync = self.party_sync();
        let spawned = service::host(
            &services.runtime,
            services.store.clone(),
            name,
            settings,
            sync,
        );
        let role = Role::Host {
            invite: None,
            settings,
        };
        self.attach_party(spawned, role, cx);
    }

    /// Join with an invite link. A link that is not one is refused here; anything that goes
    /// wrong later shows in the party's view.
    pub fn join_party(
        &mut self,
        invite: &str,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), InviteError> {
        let raw = invite.trim().to_owned();
        let invite = parse_invite(invite)?;
        let Some(services) = self.services.clone() else {
            return Ok(());
        };
        let name = name
            .and_then(|name| display_name(&name))
            .unwrap_or_else(|| self.party_name());
        let sync = self.party_sync();
        let spawned = service::member(
            &services.runtime,
            services.store.clone(),
            name.clone(),
            invite,
            sync,
        );
        self.attach_party(spawned, Role::Member, cx);
        if let Some(party) = self.party.as_mut() {
            party.invite = Some(raw);
            party.joined_as = Some(name);
        }
        Ok(())
    }

    /// Member: the connection dropped, so join again after a pause, the same as pressing
    /// Rejoin. The party keeps its place: the host sends the film and position on welcome.
    fn rejoin_later(&mut self, cx: &mut Context<Self>) {
        let Some(party) = self.party.as_mut() else {
            return;
        };
        let (Some(delay), Some(invite)) = (party.rejoin_after(), party.invite.clone()) else {
            return;
        };
        let name = party.joined_as.clone();
        let tries = party.rejoins + 1;
        super::trace(|| format!("rejoin {tries} in {}s", delay.as_secs()));
        party.view.link = super::Link::Ended(EndReason::Reconnecting);
        party._rejoin = Some(cx.spawn(async move |state, cx| {
            cx.background_executor().timer(delay).await;
            let _ = state.update(cx, |state, cx| {
                let still = state
                    .party
                    .as_ref()
                    .is_some_and(|party| party.view.ended() == Some(EndReason::Reconnecting));
                if still
                    && state.join_party(&invite, name, cx).is_ok()
                    && let Some(party) = state.party.as_mut()
                {
                    party.rejoins = tries;
                }
            });
        }));
    }

    /// Leave, or as the host, end the party for everyone. Playback carries on.
    pub fn leave_party(&mut self, cx: &mut Context<Self>) {
        let Some(party) = self.party.take() else {
            return;
        };
        if !party.view.is_host()
            && let Some(host) = &self.playback_host
        {
            host.send(Command::SetSpeed(1.0));
        }
        self.drop_party_play();
        cx.notify();
    }

    /// Forgets a film the party asked for and that has not opened yet.
    fn drop_party_play(&mut self) {
        if self
            .pending_play
            .as_ref()
            .is_some_and(|play| matches!(play.pick, Pick::Party(_)))
        {
            self.pending_play = None;
        }
    }

    /// Host only.
    pub fn set_party_settings(&mut self, settings: HostSettings, cx: &mut Context<Self>) {
        if let Some(party) = self.party.as_ref().filter(|party| party.view.is_host()) {
            party.send(Input::Settings(settings));
            cx.notify();
        }
    }

    /// The user's own play, pause or seek, while the party steers playback.
    pub fn party_action(&self, action: Action) {
        if let Some(party) = &self.party {
            party.send(Input::Local(action));
        }
    }

    /// The local player's state, on change and while playing.
    pub fn party_report(&self, state: MemberState, position: f64, duration: Option<f64>) {
        if let Some(party) = &self.party {
            party.send(Input::Report {
                state,
                position,
                duration,
            });
        }
    }

    /// Host: the local player opened `media` at `at` seconds. Announcing the same media again
    /// does nothing.
    pub fn party_media(&mut self, media: PartyMedia, at: f64) {
        if let Some(party) = self.party.as_mut().filter(|party| party.view.is_host()) {
            party.announce(media, at);
        }
    }

    /// Host: the player itself opened `media`, so the party plays it now.
    pub fn party_media_playing(&mut self, media: PartyMedia, at: f64) {
        if let Some(party) = self.party.as_mut().filter(|party| party.view.is_host()) {
            party.announce_playing(media, at);
        }
    }

    /// Host: leave the lobby. The party plays once everyone is ready, or after the host's
    /// wait runs out.
    pub fn party_start(&self) {
        if let Some(party) = self.party.as_ref().filter(|party| party.view.is_host()) {
            party.send(Input::Start);
        }
    }

    /// Member: the host closed the film, and the party waits in the lobby for the next one.
    pub fn party_stopped(&self) -> bool {
        self.party.as_ref().is_some_and(|party| {
            !party.view.is_host()
                && party.view.ended().is_none()
                && party.media.is_none()
                && party.view.gate == Some(Gate::Lobby)
        })
    }

    /// Host: the film closed. Friends close theirs and wait in the lobby, and the next
    /// announced film reaches them even when it is the same one.
    pub fn party_stop(&mut self) {
        if let Some(party) = self.party.as_mut().filter(|party| party.view.is_host())
            && party.media.take().is_some()
        {
            party.send(Input::Stop);
        }
    }

    /// Member: the player closed on `film_id`, so this person is away and holds nothing up. A
    /// last report of buffering would otherwise make everyone wait for a player that is gone.
    /// When the party has moved to another film, this player's report no longer applies.
    pub fn party_left_film(&self, film_id: &str, position: f64, duration: Option<f64>) {
        let current = self.party.as_ref().is_some_and(|party| {
            !party.view.is_host() && party.media().is_some_and(|media| media.film_id == film_id)
        });
        if current {
            self.party_report(MemberState::Away, position, duration);
        }
    }

    /// Member: whether the source now playing is the host's release.
    pub fn party_release(&self, matched: bool) {
        if let Some(party) = &self.party {
            party.send(Input::Release { matched });
        }
    }

    /// Member: the player opened a new file, so the party's timeline applies again.
    pub fn party_reopened(&self) {
        if let Some(party) = &self.party {
            party.send(Input::Reopened);
        }
    }

    /// Member: what stops the party's film playing here, if anything.
    pub fn party_blocked(&self) -> Option<Blocked> {
        let party = self
            .party
            .as_ref()
            .filter(|party| !party.view.is_host() && party.view.ended().is_none())?;
        let session = self.services.as_ref().is_some_and(|s| s.session.is_some());
        Blocked::of(
            &self.account,
            !session || self.account_known,
            session && self.installed.is_empty(),
            party.lookup_failed,
        )
        .or(party.source_failed.then_some(Blocked::SourceFailed))
    }

    /// Member: whether the player's current source has failed.
    pub fn set_party_source_failed(&mut self, failed: bool, cx: &mut Context<Self>) {
        if let Some(party) = self
            .party
            .as_mut()
            .filter(|party| !party.view.is_host() && party.source_failed != failed)
        {
            party.source_failed = failed;
            cx.notify();
        }
    }

    /// Member: keeps the host's roster honest about whether this person can play.
    pub fn sync_party_blocked(&mut self) {
        let blocked = self.party_blocked().is_some();
        let Some(party) = self.party.as_mut().filter(|party| !party.view.is_host()) else {
            return;
        };
        if party.told_blocked == blocked {
            return;
        }
        party.told_blocked = blocked;
        let state = if blocked {
            MemberState::Failed
        } else {
            MemberState::Loading
        };
        party.send(Input::Report {
            state,
            position: 0.0,
            duration: None,
        });
    }

    /// Member: looks the party's film up again.
    pub fn retry_party_film(&mut self, cx: &mut Context<Self>) {
        let Some(party) = self.party.as_mut() else {
            return;
        };
        party.lookup_failed = false;
        party.source_failed = false;
        if let Some(media) = party.media.clone() {
            self.play_by_id(
                media.kind,
                media.film_id,
                media.video_id,
                Pick::Party(media.release),
            );
        }
        cx.notify();
    }

    /// Open a film by id once the account is ready, starting with `pick`'s source.
    pub fn play_by_id(&mut self, kind: Kind, film_id: String, video_id: String, pick: Pick) {
        self.pending_play = Some(PendingPlay {
            kind,
            film_id,
            video_id,
            pick,
            stage: Stage::Waiting,
        });
    }

    /// Moves a pending play along; hands back the film, video and pick once its details are
    /// in, for the shell to launch and navigate.
    pub fn take_ready_play(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(FilmDetails, String, Pick)> {
        let ready = matches!(self.account, Account::SignedIn(_)) && !self.installed.is_empty();
        let play = self.pending_play.as_mut()?;
        match &play.stage {
            Stage::Waiting if ready => {
                let services = self.services.clone()?;
                let (job, mut receiver) = services.details(play.kind, play.film_id.clone());
                let film_id = play.film_id.clone();
                play.stage = Stage::Fetching {
                    _task: cx.spawn(async move |state, cx| {
                        let _job = job;
                        let found = receiver.next().await.and_then(|event| match event {
                            ResourceEvent::CacheHit(details) | ResourceEvent::Fresh(details) => {
                                Some(details)
                            }
                            ResourceEvent::Failed(_) => None,
                        });
                        if let Some(details) = found {
                            let _ = state.update(cx, |state, cx| {
                                if let Some(play) = state
                                    .pending_play
                                    .as_mut()
                                    .filter(|play| play.film_id == film_id)
                                {
                                    play.stage = Stage::Ready(Box::new(details));
                                    cx.notify();
                                }
                            });
                            return;
                        }
                        super::trace(|| "pending play details failed".into());
                        let _ = state.update(cx, |state, cx| {
                            state.pending_play = None;
                            if let Some(party) = state.party.as_mut() {
                                party.lookup_failed = !party.view.is_host();
                            }
                            cx.notify();
                        });
                    }),
                };
                None
            }
            Stage::Ready(_) => {
                let play = self.pending_play.take()?;
                let Stage::Ready(film) = play.stage else {
                    return None;
                };
                Some((*film, play.video_id, play.pick))
            }
            Stage::Waiting | Stage::Fetching { .. } => None,
        }
    }

    fn attach_party(&mut self, spawned: service::Spawned, role: Role, cx: &mut Context<Self>) {
        self.leave_party(cx);
        let mut events = spawned.events;
        let task = cx.spawn(async move |state, cx| {
            while let Some(event) = events.next().await {
                if state
                    .update(cx, |state, cx| state.party_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        self.party = Some(Party {
            view: PartyView::new(role, super::Link::Starting),
            inputs: spawned.inputs,
            media: None,
            host_tracks: None,
            switches: 0,
            lookup_failed: false,
            source_failed: false,
            invite: None,
            joined_as: None,
            told_blocked: false,
            rejoins: 0,
            _rejoin: None,
            _events: task,
        });
        cx.notify();
    }

    fn party_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let Some(party) = self.party.as_mut() else {
            return;
        };
        match event {
            Event::View(view) => {
                let ended = view.ended().is_some();
                if view.link == super::Link::Live {
                    party.rejoins = 0;
                }
                party.view = view;
                if ended {
                    self.drop_party_play();
                    self.rejoin_later(cx);
                }
            }
            Event::Player(command) => {
                if let Some(host) = &self.playback_host {
                    host.send(command);
                }
            }
            Event::Tracks(tracks) => party.host_tracks = Some(tracks),
            Event::Stop => {
                party.lookup_failed = false;
                party.source_failed = false;
                party.told_blocked = false;
                party.host_tracks = None;
                party.media = None;
                self.drop_party_play();
            }
            Event::Load(media) => {
                if super::switched_source(party.media.as_ref(), &media) {
                    party.switches += 1;
                }
                party.lookup_failed = false;
                party.source_failed = false;
                party.told_blocked = false;
                party.host_tracks = None;
                party.media = Some(media.clone());
                self.play_by_id(
                    media.kind,
                    media.film_id,
                    media.video_id,
                    Pick::Party(media.release),
                );
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::party::{Link, PartyView};
    use panorama_core::party::{HostInput, HostSession, PartyRelease, transport::PartySecret};

    fn media() -> PartyMedia {
        PartyMedia {
            film_id: "tt1".into(),
            kind: Kind::Movie,
            video_id: "tt1".into(),
            title: "Film".into(),
            release: PartyRelease {
                addon: "com.host.addon".into(),
                torrent: None,
                label: "Film 1080p".into(),
                binge_group: None,
            },
        }
    }

    fn hosting() -> (Party, tokio::sync::mpsc::UnboundedReceiver<Input>) {
        let (inputs, received) = tokio::sync::mpsc::unbounded_channel();
        let role = Role::Host {
            invite: None,
            settings: HostSettings::default(),
        };
        let party = Party {
            view: PartyView::new(role, Link::Live),
            inputs,
            media: None,
            host_tracks: None,
            switches: 0,
            lookup_failed: false,
            source_failed: false,
            invite: None,
            joined_as: None,
            told_blocked: false,
            rejoins: 0,
            _rejoin: None,
            _events: Task::ready(()),
        };
        (party, received)
    }

    fn gate_after(mut received: tokio::sync::mpsc::UnboundedReceiver<Input>) -> Gate {
        let secret = PartySecret::generate().unwrap();
        let mut session = HostSession::new("Host".into(), secret, HostSettings::default());
        while let Ok(input) = received.try_recv() {
            match input {
                Input::Media { media, at } => {
                    session.handle(1_000, HostInput::SetMedia { media, at });
                }
                Input::Start => {
                    session.handle(1_000, HostInput::Start);
                }
                other => panic!("unexpected input {other:?}"),
            }
        }
        session.gate()
    }

    fn member_watching(film: &str) -> Party {
        let (mut party, _) = hosting();
        party.view.role = Role::Member;
        party.media = Some(PartyMedia {
            film_id: film.into(),
            video_id: film.into(),
            ..media()
        });
        party
    }

    #[test]
    fn a_friend_is_told_the_one_thing_stopping_them() {
        let signed_in = Account::SignedIn("a@b.c".into());
        assert_eq!(
            Blocked::of(&Account::SignedOut, true, true, false),
            Some(Blocked::SignedOut)
        );
        assert_eq!(Blocked::of(&Account::SignedOut, false, true, false), None);
        assert_eq!(
            Blocked::of(&Account::Authenticating, true, true, false),
            None
        );
        assert_eq!(
            Blocked::of(&signed_in, true, true, false),
            Some(Blocked::NoAddons)
        );
        assert_eq!(Blocked::of(&signed_in, true, false, false), None);
        assert_eq!(
            Blocked::of(&signed_in, true, false, true),
            Some(Blocked::FilmNotFound)
        );
    }

    #[test]
    fn only_a_dropped_connection_offers_rejoin() {
        let mut party = member_watching("tt1");
        party.invite = Some("panorama://party/x".into());
        party.view.link = Link::Ended(EndReason::Lost);
        assert_eq!(party.rejoin_invite(), Some("panorama://party/x"));
        party.view.link = Link::Ended(EndReason::Left);
        assert_eq!(party.rejoin_invite(), None);
        party.view.link = Link::Live;
        assert_eq!(party.rejoin_invite(), None);
    }

    #[test]
    fn a_dropped_friend_rejoins_on_their_own_three_times_then_by_hand() {
        let mut party = member_watching("tt1");
        party.invite = Some("panorama://party/x".into());
        party.view.link = Link::Ended(EndReason::Lost);
        let delays: Vec<u64> = (0..4)
            .map_while(|tries| {
                party.rejoins = tries;
                party.rejoin_after().map(|delay| delay.as_secs())
            })
            .collect();
        assert_eq!(delays, [2, 5, 10]);
        assert_eq!(party.rejoin_invite(), Some("panorama://party/x"));
        party.view.link = Link::Ended(EndReason::Reconnecting);
        assert_eq!(party.rejoin_invite(), None);
        party.view.link = Link::Ended(EndReason::Left);
        party.rejoins = 0;
        assert_eq!(party.rejoin_after(), None);
    }

    #[test]
    fn the_pill_counts_people_only_once_there_are_friends() {
        let (mut party, _) = hosting();
        assert_eq!(party.pill_label(), "Watch party");
        assert!(!party.plays_for_everyone());
        party.view = crate::party_lobby::preview(true);
        assert_eq!(party.pill_label(), "Watch party · 4");
        assert!(party.plays_for_everyone());
    }

    #[test]
    fn only_another_film_pulls_a_friend_away() {
        let party = member_watching("tt1");
        assert!(!party.pulls_friend_away("tt1", "tt1"));
        assert!(party.pulls_friend_away("tt2", "tt2"));
        assert!(party.pulls_friend_away("tt1", "tt1:1:2"));
    }

    #[test]
    fn a_host_is_never_asked_to_leave_their_own_party() {
        let (party, _) = hosting();
        assert!(!party.pulls_friend_away("tt2", "tt2"));
    }

    #[test]
    fn a_film_the_hosts_player_opens_leaves_the_lobby() {
        let (mut party, received) = hosting();
        party.announce_playing(media(), 0.0);
        assert_eq!(gate_after(received), Gate::Ready);
    }

    #[test]
    fn a_film_announced_from_the_lobby_waits_for_start() {
        let (mut party, received) = hosting();
        party.announce(media(), 0.0);
        assert_eq!(gate_after(received), Gate::Lobby);
    }
}
