//! The watch party lobby: who is in, what plays, and the host's Start, shown over the app
//! before the party's film opens.
mod source_rows;
mod view;

use crate::{
    app::AppShell,
    app_state::AppState,
    party::{PartyView, RosterRow},
    playback::selection::{PlaybackSelection, SourceRef},
    theme::motion::*,
    transition::Tween,
};
use gpui::{Context, Entity, FocusHandle, ScrollHandle, Subscription, WeakEntity, Window};
use panorama_core::{
    addons::{FilmDetails, StreamState},
    party::{Gate, MemberState, PartyMedia, PartyRelease},
};
use std::time::Instant;

gpui::actions!(panorama_lobby, [NextControl, PreviousControl, CloseLobby]);

/// Whose lobby this is. The host's carries what Start watching plays.
pub enum Side {
    Host(Box<PlaybackSelection>),
    Member,
}

/// What the user asked of the lobby.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Escape,
    StartWatching,
    StartAnyway,
    /// Host: back to browsing for another film, keeping the party.
    PickAnotherFilm,
    /// Leave the party, or as the host, end it for everyone.
    LeaveParty,
    /// Member: the host started the party.
    HostStarted,
}

/// How the lobby closed, decided once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Exit {
    /// Host: the party leaves the lobby and the player opens.
    Start,
    /// Leave the party, or as the host, end it for everyone.
    Leave,
    /// Host: the lobby closes and the party waits, with friends told a film is being chosen.
    Browse,
    /// Member: the party began, so the player under the lobby shows.
    Started,
}

/// Who the host would wait for, in roster order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Readiness {
    pub waiting: Vec<String>,
    /// Those waited for whose player can't play the source: only Change or Start anyway helps.
    pub failed: Vec<String>,
    pub friends: usize,
}

impl Readiness {
    pub fn of(roster: &[RosterRow]) -> Self {
        let friends = roster.iter().filter(|row| !row.member.is_host);
        Self {
            friends: friends.clone().count(),
            waiting: friends
                .clone()
                .filter(|row| !is_ready(row.member.state))
                .map(|row| row.member.name.clone())
                .collect(),
            failed: friends
                .filter(|row| row.member.state == MemberState::Failed)
                .map(|row| row.member.name.clone())
                .collect(),
        }
    }

    /// Start watching: every friend who joined has the film ready.
    pub fn can_start(&self) -> bool {
        self.waiting.is_empty()
    }

    /// Start anyway: offered while someone is still getting there.
    pub fn offers_start_anyway(&self) -> bool {
        !self.waiting.is_empty()
    }
}

/// A member is ready once its player holds the film, paused or not.
pub fn is_ready(state: MemberState) -> bool {
    matches!(
        state,
        MemberState::Ready | MemberState::Paused | MemberState::Playing
    )
}

/// Member: the party left the lobby and the host's film is open, so the friend's shows. While
/// the host's player is still opening it, the friend waits here rather than on a paused frame.
pub fn host_started(view: &PartyView) -> bool {
    view.gate.is_some_and(|gate| gate != Gate::Lobby)
        // Only a listed host counts: a friend joining mid-start hears the state first.
        && view.roster.iter().any(|row| {
            row.member.is_host
                && !matches!(
                    row.member.state,
                    MemberState::Loading | MemberState::Buffering
                )
        })
}

/// What `command` does from this side, given who is ready. `None` leaves the lobby open.
pub fn decide(host: bool, readiness: &Readiness, command: Command) -> Option<Exit> {
    match command {
        // A host with friends in keeps them: closing goes back to choosing a film.
        Command::Escape if host && readiness.friends > 0 => Some(Exit::Browse),
        Command::Escape | Command::LeaveParty => Some(Exit::Leave),
        Command::PickAnotherFilm if host => Some(Exit::Browse),
        Command::StartWatching if host && readiness.can_start() => Some(Exit::Start),
        Command::StartAnyway if host && readiness.offers_start_anyway() => Some(Exit::Start),
        Command::HostStarted if !host => Some(Exit::Started),
        _ => None,
    }
}

/// The party lobby dialog, presented over the shell like the sign-in form.
pub struct PartyLobby {
    shell: WeakEntity<AppShell>,
    state: Entity<AppState>,
    side: Side,
    /// The film for the header when the party has not loaded one here yet: a preview's.
    film: Option<FilmDetails>,
    root: FocusHandle,
    close: FocusHandle,
    copy: FocusHandle,
    change: FocusHandle,
    pick_film: FocusHandle,
    sources: Vec<FocusHandle>,
    subtitles: FocusHandle,
    audio: FocusHandle,
    start_anyway: FocusHandle,
    start: FocusHandle,
    leave: FocusHandle,
    scroll: ScrollHandle,
    /// The host's Change list is open.
    changing: bool,
    copied: bool,
    presence: Tween,
    scrim: Tween,
    exit: Option<Exit>,
    art: Option<url::Url>,
    _state: Subscription,
    _images: Subscription,
}

impl PartyLobby {
    pub fn new(
        shell: WeakEntity<AppShell>,
        state: Entity<AppState>,
        side: Side,
        film: Option<FilmDetails>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observer = cx.observe_in(&state, window, |lobby, _, window, cx| {
            lobby.follow_party(window, cx);
            cx.notify();
        });
        let images = state.read(cx).images.clone();
        let images = cx.observe(&images, |_, _, cx| cx.notify());
        let mut presence = Tween::fixed(0.0);
        presence.retarget(1.0, DURATION_STANDARD, Some(EASE_EDITORIAL), Instant::now());
        let mut scrim = Tween::fixed(0.0);
        scrim.retarget(1.0, DURATION_FAST, None, Instant::now());
        let root = cx.focus_handle().tab_stop(false);
        root.focus(window, cx);
        let lobby = Self {
            shell,
            state,
            side,
            film,
            root,
            close: cx.focus_handle(),
            copy: cx.focus_handle(),
            change: cx.focus_handle(),
            pick_film: cx.focus_handle(),
            sources: Vec::new(),
            subtitles: cx.focus_handle(),
            audio: cx.focus_handle(),
            start_anyway: cx.focus_handle(),
            start: cx.focus_handle(),
            leave: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            changing: false,
            copied: false,
            presence,
            scrim,
            exit: None,
            art: None,
            _state: observer,
            _images: images,
        };
        lobby.announce(cx);
        lobby
    }

    fn is_host(&self) -> bool {
        matches!(self.side, Side::Host(_))
    }

    fn party<'a>(&self, cx: &'a gpui::App) -> Option<&'a PartyView> {
        self.state.read(cx).party.as_ref().map(|party| &party.view)
    }

    fn readiness(&self, cx: &gpui::App) -> Readiness {
        self.party(cx)
            .map_or_else(Readiness::default, |view| Readiness::of(&view.roster))
    }

    /// The film the header shows: the host's pick, or for a member, the film the party loaded.
    fn header_film(&self, cx: &gpui::App) -> Option<FilmDetails> {
        if let Side::Host(selection) = &self.side {
            return Some(selection.film.clone());
        }
        let state = self.state.read(cx);
        let media = state.party.as_ref().and_then(|party| party.media());
        state
            .playback
            .as_ref()
            .map(|playing| &playing.film)
            .filter(|film| media.is_some_and(|media| media.film_id == film.id))
            .cloned()
            .or_else(|| self.film.clone())
    }

    /// Host: tell the party what it will watch, so friends load it while the lobby is open.
    /// The host counts as ready: it picked the source.
    fn announce(&self, cx: &mut Context<Self>) {
        let Side::Host(selection) = &self.side else {
            return;
        };
        let Some(media) = media_of(selection) else {
            return;
        };
        let at = selection.start.seconds();
        self.state.update(cx, |state, _| {
            state.party_media(media, at);
            state.party_report(MemberState::Ready, at, None);
        });
    }

    /// Host: play another source; friends reload the new release.
    fn choose_source(&mut self, source: SourceRef, cx: &mut Context<Self>) {
        if let Side::Host(selection) = &mut self.side {
            selection.source = source;
        }
        self.changing = false;
        self.announce(cx);
        cx.notify();
    }

    fn follow_party(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let started = self.party(cx).is_some_and(host_started);
        if self.state.read(cx).party.is_none() {
            self.leave_quietly(window, cx);
        } else if started {
            self.act(Command::HostStarted, window, cx);
        }
    }

    /// The party went away elsewhere; there is nothing left to leave.
    fn leave_quietly(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.exit.is_none() {
            self.exit = Some(Exit::Leave);
            self.dismiss(cx);
        }
    }

    pub fn act(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.exit.is_some() {
            return;
        }
        if command == Command::LeaveParty && self.is_host() && self.readiness(cx).friends > 0 {
            let shell = self.shell.clone();
            let _ = shell.update(cx, |shell, cx| shell.leave_or_ask(window, cx));
            return;
        }
        let Some(exit) = decide(self.is_host(), &self.readiness(cx), command) else {
            return;
        };
        self.exit = Some(exit);
        match exit {
            Exit::Start => self.start(window, cx),
            Exit::Leave => {
                self.state.update(cx, |state, cx| state.leave_party(cx));
                let shell = self.shell.clone();
                let _ = shell.update(cx, |shell, cx| shell.leave_party_route(window, cx));
            }
            Exit::Browse => self.state.update(cx, |state, _| state.party_stop()),
            Exit::Started => {}
        }
        self.dismiss(cx);
        cx.stop_propagation();
    }

    /// Host: the party leaves the lobby, held on the host's own player loading, and the
    /// player opens on the chosen source.
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Side::Host(selection) = &self.side else {
            return;
        };
        let selection = (**selection).clone();
        let id = selection.film.id.clone();
        let at = selection.start.seconds();
        self.state.update(cx, |state, _| {
            state.party_report(MemberState::Loading, at, None);
            state.party_start();
            state.playback = Some(selection);
        });
        let shell = self.shell.clone();
        let _ = shell.update(cx, |shell, cx| {
            shell.navigate(crate::router::Route::Player { id }, window, cx)
        });
    }

    /// Member: the way out of what stops the film playing here.
    fn fix(&mut self, blocked: crate::party::Blocked, window: &mut Window, cx: &mut Context<Self>) {
        match blocked {
            crate::party::Blocked::SignedOut => {
                let shell = self.shell.clone();
                let _ = shell.update(cx, |shell, cx| shell.open_login(window, cx));
            }
            crate::party::Blocked::FilmNotFound | crate::party::Blocked::SourceFailed => {
                self.state
                    .update(cx, |state, cx| state.retry_party_film(cx));
            }
            crate::party::Blocked::NoAddons => {}
        }
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.presence
            .retarget(0.0, DURATION_FAST, Some(EASE_EDITORIAL), Instant::now());
        self.scrim
            .retarget(0.0, DURATION_FAST, None, Instant::now());
        cx.notify();
    }

    /// The tab order, top to bottom, of the controls showing now.
    fn tab_order(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        let mut order = vec![self.close.clone()];
        match &self.side {
            Side::Host(_) => {
                if invite(self.party(cx)).is_some() {
                    order.push(self.copy.clone());
                }
                order.push(self.change.clone());
                if self.changing {
                    order.extend(self.sources.iter().cloned());
                }
                order.push(self.pick_film.clone());
                order.push(self.leave.clone());
                let readiness = self.readiness(cx);
                if readiness.offers_start_anyway() {
                    order.push(self.start_anyway.clone());
                }
                if readiness.can_start() {
                    order.push(self.start.clone());
                }
            }
            Side::Member => {
                order.push(self.subtitles.clone());
                order.push(self.audio.clone());
                order.push(self.leave.clone());
                let state = self.state.read(cx);
                if state
                    .party_blocked()
                    .is_some_and(|blocked| blocked != crate::party::Blocked::NoAddons)
                    || state
                        .party
                        .as_ref()
                        .is_some_and(|party| party.rejoin_invite().is_some())
                {
                    order.push(self.start.clone());
                }
            }
        }
        order
    }

    fn cycle(&self, backward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let handles = self.tab_order(cx);
        let next = match handles.iter().position(|handle| handle.is_focused(window)) {
            Some(current) if backward => (current + handles.len() - 1) % handles.len(),
            Some(current) => (current + 1) % handles.len(),
            None if backward => handles.len() - 1,
            None => 0,
        };
        handles[next].focus(window, cx);
        cx.stop_propagation();
    }
}

/// The host's invite, as the party hands it out.
fn invite(view: Option<&PartyView>) -> Option<String> {
    match &view?.role {
        crate::party::Role::Host { invite, .. } => invite.as_ref().map(|link| link.as_str().into()),
        crate::party::Role::Member => None,
    }
}

/// What the party watches when the host plays `selection`.
fn media_of(selection: &PlaybackSelection) -> Option<PartyMedia> {
    let group = selection.groups.get(selection.source.group)?;
    let StreamState::Ready(streams) = &group.state else {
        return None;
    };
    Some(PartyMedia {
        film_id: selection.film.id.clone(),
        kind: selection.film.kind,
        video_id: selection.video_id.clone(),
        title: selection.film.name.chars().take(200).collect(),
        release: PartyRelease::of(group, streams.get(selection.source.stream)?),
    })
}

/// A made-up lobby for `--open-party-lobby` captures: the host, one friend ready and one
/// still loading.
pub fn preview(host: bool) -> PartyView {
    use crate::party::{InviteLink, Link, Role};
    use panorama_core::party::{HostSettings, MemberView};
    let person = |member_id, name: &str, state, is_host| RosterRow {
        member: MemberView {
            member_id,
            name: name.into(),
            state,
            late: false,
            release_matched: true,
            is_host,
        },
        path: None,
    };
    PartyView {
        role: if host {
            Role::Host {
                invite: Some(InviteLink::preview()),
                settings: HostSettings::default(),
            }
        } else {
            Role::Member
        },
        link: Link::Live,
        roster: vec![
            person(0, "Billy", MemberState::Ready, true),
            person(1, "Sam", MemberState::Paused, false),
            person(2, "Rina", MemberState::Loading, false),
        ],
        anyone_controls: false,
        notices: Vec::new(),
        sync: crate::party::PartySync {
            subtitles: true,
            audio: false,
        },
        me: (!host).then_some(1),
        gate: (!host).then_some(Gate::Lobby),
    }
}

#[cfg(test)]
mod tests;
