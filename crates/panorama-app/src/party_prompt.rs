//! The one question a watch party asks before an action would change it for someone.
use crate::{
    app::AppShell,
    button::Button,
    party_lobby::{CloseLobby, NextControl, PreviousControl},
    playback::{launch::Pick, selection::PlaybackSelection},
    router::Route,
    theme::{Theme, Typography, motion::*},
    transition::Tween,
};
use gpui::{
    BoxShadow, Context, FocusHandle, Render, WeakEntity, Window, div, point, prelude::*, px,
};
use panorama_core::{addons::FilmDetails, stremio::ResumePoint};
use std::time::Instant;

const WIDTH: f32 = 440.0;

/// A film to open, as whichever screen asked for it knows it.
pub enum Play {
    /// Sources are already picked.
    Selection(Box<PlaybackSelection>),
    /// The player looks the sources up.
    Launch {
        film: Box<FilmDetails>,
        video_id: String,
        resume: Option<ResumePoint>,
    },
}

impl Play {
    fn film(&self) -> &FilmDetails {
        match self {
            Self::Selection(selection) => &selection.film,
            Self::Launch { film, .. } => film,
        }
    }

    fn video_id(&self) -> &str {
        match self {
            Self::Selection(selection) => &selection.video_id,
            Self::Launch { video_id, .. } => video_id,
        }
    }
}

/// What the party needs an answer on, holding what happens on yes.
pub enum Ask {
    /// A friend is opening something other than the party's film.
    LeaveToPlay(Play),
    /// A friend is starting a party of their own.
    LeaveToHost(Box<PlaybackSelection>),
    /// The host is closing the film everyone is watching.
    StopFilm,
    /// The host is ending the party with friends in it.
    EndParty,
    /// An invite arrived while already in a party.
    JoinOther { invite: String, hosting: bool },
    /// The host started a film while this friend was browsing.
    JoinFilm(Box<(FilmDetails, String, Pick)>),
}

/// The words of one question.
#[derive(Debug, Eq, PartialEq)]
pub struct Wording {
    pub title: String,
    pub body: String,
    pub confirm: &'static str,
    pub stay: &'static str,
}

impl Ask {
    pub fn wording(&self) -> Wording {
        match self {
            Self::LeaveToPlay(play) => Wording {
                title: "Leave the party?".into(),
                body: format!(
                    "{} will play just for you. The party keeps going, and you can rejoin with the invite link.",
                    play.film().name
                ),
                confirm: "Leave and play",
                stay: "Stay in party",
            },
            Self::LeaveToHost(selection) => Wording {
                title: "Leave the party?".into(),
                body: format!(
                    "You'll start a new party for {}. The one you're in keeps going without you.",
                    selection.film.name
                ),
                confirm: "Leave and host",
                stay: "Stay in party",
            },
            Self::StopFilm => Wording {
                title: "Stop the film for everyone?".into(),
                body: "Your friends go back to the lobby while you pick what's next. The party stays open."
                    .into(),
                confirm: "Stop film",
                stay: "Keep watching",
            },
            Self::EndParty => Wording {
                title: "End the party?".into(),
                body: "Everyone is disconnected and the invite link stops working. Friends can keep watching on their own."
                    .into(),
                confirm: "End party",
                stay: "Keep party",
            },
            Self::JoinOther { hosting: true, .. } => Wording {
                title: "End your party and join?".into(),
                body: "Joining this invite ends the party you're hosting for everyone in it.".into(),
                confirm: "End and join",
                stay: "Keep my party",
            },
            Self::JoinOther { hosting: false, .. } => Wording {
                title: "Switch parties?".into(),
                body: "Joining this invite takes you out of the party you're in.".into(),
                confirm: "Switch",
                stay: "Stay in party",
            },
            Self::JoinFilm(next) => Wording {
                title: format!("{} is on", next.0.name),
                body: "The host started a new film for the party.".into(),
                confirm: "Join",
                stay: "Not now",
            },
        }
    }
}

/// The dialog over the shell. Escape, like Stay, changes nothing.
pub struct PartyPrompt {
    shell: WeakEntity<AppShell>,
    ask: Option<Ask>,
    wording: Wording,
    root: FocusHandle,
    stay: FocusHandle,
    confirm: FocusHandle,
    presence: Tween,
}

impl PartyPrompt {
    fn new(
        shell: WeakEntity<AppShell>,
        ask: Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let stay = cx.focus_handle();
        stay.focus(window, cx);
        let mut presence = Tween::fixed(0.0);
        presence.retarget(1.0, DURATION_FAST, Some(EASE_EDITORIAL), Instant::now());
        Self {
            shell,
            wording: ask.wording(),
            ask: Some(ask),
            root: cx.focus_handle().tab_stop(false),
            stay,
            confirm: cx.focus_handle(),
            presence,
        }
    }

    fn answer(&mut self, yes: bool, window: &mut Window, cx: &mut Context<Self>) {
        let ask = self.ask.take().filter(|_| yes);
        let shell = self.shell.clone();
        let _ = shell.update(cx, |shell, cx| shell.answer_party_prompt(ask, window, cx));
        cx.stop_propagation();
    }

    fn cycle(&self, window: &mut Window, cx: &mut Context<Self>) {
        let next = if self.stay.is_focused(window) {
            &self.confirm
        } else {
            &self.stay
        };
        next.focus(window, cx);
        cx.stop_propagation();
    }
}

impl Render for PartyPrompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (theme, reduced, keyboard) = self
            .shell
            .upgrade()
            .map(|shell| {
                let shell = shell.read(cx);
                (shell.theme, shell.reduced_motion, shell.keyboard_navigation)
            })
            .unwrap_or((Theme::default(), false, true));
        let now = Instant::now();
        let progress = self.presence.value(now, reduced);
        if self.presence.moving(now, reduced) {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        let width = (f32::from(window.viewport_size().width) - 40.0).min(WIDTH);
        let panel =
            div()
                .relative()
                .w(px(width))
                .px(px(28.0))
                .pt(px(26.0))
                .pb(px(22.0))
                .flex()
                .flex_col()
                .bg(theme.surface)
                .text_color(theme.ink)
                .opacity(progress)
                .top(px(if reduced { 0.0 } else { 8.0 * (1.0 - progress) }))
                .shadow(vec![BoxShadow {
                    inset: false,
                    color: theme.shadow.into(),
                    offset: point(px(0.0), px(24.0)),
                    blur_radius: px(80.0),
                    spread_radius: px(0.0),
                }])
                .child(div().title().child(self.wording.title.clone()))
                .child(
                    div()
                        .mt(px(8.0))
                        .label()
                        .text_color(theme.ink_muted)
                        .child(self.wording.body.clone()),
                )
                .child(
                    div()
                        .mt(px(24.0))
                        .flex()
                        .flex_wrap()
                        .justify_end()
                        .gap(px(8.0))
                        .child(
                            Button::glass("party-prompt-stay", self.wording.stay)
                                .compact()
                                .focus(&self.stay, keyboard)
                                .on_click(cx.listener(|prompt, _, window, cx| {
                                    prompt.answer(false, window, cx)
                                })),
                        )
                        .child(
                            Button::primary("party-prompt-confirm", self.wording.confirm)
                                .compact()
                                .reduced_motion(reduced)
                                .focus(&self.confirm, keyboard)
                                .on_click(cx.listener(|prompt, _, window, cx| {
                                    prompt.answer(true, window, cx)
                                })),
                        ),
                );
        div()
            .id("party-prompt")
            .absolute()
            .inset_0()
            .occlude()
            .key_context("PanoramaLobby")
            .track_focus(&self.root)
            .role(gpui::Role::AlertDialog)
            .aria_label(self.wording.title.clone())
            .on_action(cx.listener(|prompt, _: &NextControl, window, cx| prompt.cycle(window, cx)))
            .on_action(
                cx.listener(|prompt, _: &PreviousControl, window, cx| prompt.cycle(window, cx)),
            )
            .on_action(
                cx.listener(|prompt, _: &CloseLobby, window, cx| prompt.answer(false, window, cx)),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(theme.scrim.opacity(0.72))
                    .opacity(progress),
            )
            .child(
                div()
                    .relative()
                    .size_full()
                    .pt(px(crate::titlebar::SAFE_TOP))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(panel),
            )
    }
}

impl AppShell {
    /// Opens `play`, first asking a friend whose party is watching something else.
    pub(crate) fn play_or_ask(&mut self, play: Play, window: &mut Window, cx: &mut Context<Self>) {
        let elsewhere = self
            .state
            .read(cx)
            .party
            .as_ref()
            .is_some_and(|party| party.pulls_friend_away(&play.film().id, play.video_id()));
        if elsewhere {
            self.ask_party(Ask::LeaveToPlay(play), window, cx);
        } else {
            self.play_now(play, window, cx);
        }
    }

    /// Hosts a party on `selection`, first asking a friend already in someone else's.
    pub(crate) fn watch_together(
        &mut self,
        selection: PlaybackSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let in_friends_party = self
            .state
            .read(cx)
            .party
            .as_ref()
            .is_some_and(|party| !party.view.is_host() && party.view.ended().is_none());
        if in_friends_party {
            self.ask_party(Ask::LeaveToHost(Box::new(selection)), window, cx);
        } else {
            self.host_party(selection, window, cx);
        }
    }

    fn host_party(
        &mut self,
        selection: PlaybackSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state
            .update(cx, |state, cx| state.start_party(None, cx));
        self.open_party_lobby_as_host(selection, window, cx);
    }

    /// Whether the host would be cutting friends off by closing or ending now.
    fn host_with_friends(&self, cx: &gpui::App) -> bool {
        self.state.read(cx).party.as_ref().is_some_and(|party| {
            party.view.is_host()
                && party.view.ended().is_none()
                && party.view.roster.iter().any(|row| !row.member.is_host)
        })
    }

    /// The host is leaving the player: `true` when that needs a yes first.
    pub(crate) fn ask_before_closing_film(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let asks = self.host_with_friends(cx)
            && self
                .state
                .read(cx)
                .party
                .as_ref()
                .is_some_and(|party| party.media().is_some());
        if asks {
            self.ask_party(Ask::StopFilm, window, cx);
        }
        asks
    }

    /// Leaves the party, asking first when that ends it for friends.
    pub(crate) fn leave_or_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.host_with_friends(cx) {
            self.ask_party(Ask::EndParty, window, cx);
        } else {
            self.state.update(cx, |state, cx| state.leave_party(cx));
        }
    }

    pub(crate) fn ask_party(&mut self, ask: Ask, window: &mut Window, cx: &mut Context<Self>) {
        let shell = cx.weak_entity();
        self.prompt = Some(cx.new(|cx| PartyPrompt::new(shell, ask, window, cx)));
        cx.notify();
    }

    fn play_now(&mut self, play: Play, window: &mut Window, cx: &mut Context<Self>) {
        let id = play.film().id.clone();
        self.state.update(cx, |state, cx| match play {
            Play::Selection(selection) => state.playback = Some(*selection),
            Play::Launch {
                film,
                video_id,
                resume,
            } => state.launch(*film, video_id, resume, Default::default(), cx),
        });
        self.navigate(Route::Player { id }, window, cx);
    }

    /// Closes the question, and on yes does what it held.
    fn answer_party_prompt(
        &mut self,
        ask: Option<Ask>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompt = None;
        window.focus(&self.focus, cx);
        match ask {
            Some(Ask::LeaveToPlay(play)) => {
                self.state.update(cx, |state, cx| state.leave_party(cx));
                self.play_now(play, window, cx);
            }
            Some(Ask::LeaveToHost(selection)) => self.host_party(*selection, window, cx),
            Some(Ask::StopFilm) => {
                if window.is_fullscreen() {
                    window.toggle_fullscreen();
                }
                self.leave_party_route(window, cx);
            }
            Some(Ask::EndParty) => self.state.update(cx, |state, cx| state.leave_party(cx)),
            Some(Ask::JoinOther { invite, .. }) => {
                if let Err(error) = self.join_party_now(&invite, None, window, cx) {
                    crate::debug::log(format!("party join refused: {error}"));
                }
            }
            Some(Ask::JoinFilm(next)) => {
                let (film, video_id, pick) = *next;
                let id = film.id.clone();
                self.state
                    .update(cx, |state, cx| state.launch(film, video_id, None, pick, cx));
                self.navigate(Route::Player { id }, window, cx);
            }
            None => cx.notify(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaving_to_play_names_the_film_and_keeps_stay_available() {
        let film = crate::fixtures::films().remove(0);
        let name = film.name.clone();
        let ask = Ask::LeaveToPlay(Play::Launch {
            video_id: film.id.clone(),
            film: Box::new(film),
            resume: None,
        });
        let wording = ask.wording();
        assert_eq!(wording.title, "Leave the party?");
        assert!(
            wording
                .body
                .starts_with(&format!("{name} will play just for you."))
        );
        assert_eq!(wording.confirm, "Leave and play");
        assert_eq!(wording.stay, "Stay in party");
    }
}
