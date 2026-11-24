//! The retained window shell and route presentation.
use crate::{
    account_menu::AccountMenu,
    app_state::{Account, AppState},
    args::Args,
    assets, debug,
    join_party::JoinParty,
    login::LoginDialog,
    router::{Destination, History, Route},
    routes,
    sidebar::Sidebar,
    theme::Theme,
    titlebar::Titlebar,
};
use gpui::{
    AnyView, Context, Entity, FocusHandle, KeyDownEvent, NavigationDirection, Render, Task, Window,
    div, prelude::*, px,
};
use gpui_component::WindowExt;
use std::{
    collections::VecDeque,
    rc::Rc,
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

#[path = "app_controls.rs"]
mod controls;
#[path = "app_navigation.rs"]
mod navigation;
#[path = "app_view.rs"]
mod view;

struct Mounted {
    id: u64,
    route: Route,
    view: AnyView,
}

/// App-owned navigation, theme and retained view state.
pub struct AppShell {
    /// The active design palette.
    pub theme: Theme,
    /// Explicit reduced-motion policy.
    pub reduced_motion: bool,
    /// Whether focus indicators belong to keyboard interaction.
    pub keyboard_navigation: bool,
    /// Shared account and service state.
    pub state: Entity<AppState>,
    /// Account popup retained during its exit animation.
    pub menu: AccountMenu,
    /// The sidebar's Join watch party popover.
    pub(crate) join: JoinParty,
    pub(crate) sidebar: Sidebar,
    login: Option<Entity<LoginDialog>>,
    lobby: Option<Entity<crate::party_lobby::PartyLobby>>,
    /// A question the party needs answered, over everything else.
    pub(crate) prompt: Option<Entity<crate::party_prompt::PartyPrompt>>,
    /// The friend asked to go back to the party, so its film opens without asking.
    follow_next_film: bool,
    party_pill: FocusHandle,
    args: Args,
    viewport: gpui::Size<gpui::Pixels>,
    _state: gpui::Subscription,
    pub(crate) history: History,
    cache: VecDeque<Rc<Mounted>>,
    mounted: Option<Rc<Mounted>>,
    /// A card growing into its film page, over the route that is still mounted.
    zoom: Option<crate::zoom::Zoom>,
    /// The card the open film page grew from.
    returning: Option<crate::zoom::Returning>,
    pub(crate) focus: FocusHandle,
    _keystrokes: gpui::Subscription,
    titlebar: Titlebar,
    launched: Instant,
    screenshot: Option<std::path::PathBuf>,
    capture_scheduled: bool,
    player_transparent: bool,
    outcome: Sender<Result<(), String>>,
    _party: gpui::Subscription,
    invite_written: bool,
    _handoff: Task<()>,
}

impl AppShell {
    /// Mount the initial destination after fonts and the component theme are ready.
    pub fn new(
        args: Args,
        state: Entity<AppState>,
        outcome: Sender<Result<(), String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle().tab_stop(false);
        window.focus(&focus, cx);
        let window_handle = window.window_handle();
        let keystrokes = cx.observe_keystrokes(move |shell, event, window, cx| {
            if window.window_handle() == window_handle && event.keystroke.key == "tab" {
                shell.keyboard_navigation = true;
                cx.notify();
            }
        });
        let state_observer = cx.observe(&state, |_, _, cx| cx.notify());
        let party_observer = cx.observe_in(&state, window, |shell, _, window, cx| {
            shell.follow_party(window, cx);
        });
        let handoff = cx.spawn_in(window, async move |shell, cx| {
            loop {
                cx.background_executor()
                    .timer(crate::party::handoff::POLL)
                    .await;
                if shell
                    .update_in(cx, |shell, window, cx| shell.take_handoff(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let theme = Theme::default();
        let mut shell = Self {
            theme,
            reduced_motion: args.reduced_motion,
            keyboard_navigation: false,
            state: state.clone(),
            menu: AccountMenu::new(cx),
            join: JoinParty::new(cx),
            sidebar: Sidebar::new(cx),
            login: None,
            lobby: None,
            prompt: None,
            follow_next_film: false,
            party_pill: cx.focus_handle(),
            args: args.clone(),
            viewport: window.viewport_size(),
            _state: state_observer,
            history: History::new(args.route),
            cache: VecDeque::new(),
            mounted: None,
            zoom: None,
            returning: None,
            focus,
            _keystrokes: keystrokes,
            titlebar: Titlebar::new(cx, state.clone()),
            launched: Instant::now(),
            screenshot: args.screenshot,
            capture_scheduled: false,
            player_transparent: false,
            outcome,
            _party: party_observer,
            invite_written: false,
            _handoff: handoff,
        };
        let close_state = shell.state.clone();
        window.on_window_should_close(cx, move |window, cx| {
            if close_state.read(cx).playback_host.is_none() {
                return true;
            }
            crate::playback::window::close(close_state.clone(), window, cx);
            false
        });
        shell.present(window, cx);
        if args.signed_out && args.fixtures {
            shell.state.update(cx, |state, cx| {
                state.account = Account::SignedOut;
                cx.notify();
            });
        }
        if let Some(problem) = args.runtime_problem {
            shell.state.update(cx, |state, cx| {
                state.runtime_problem = Some(problem);
                cx.notify();
            });
        }
        if args.update_ready {
            shell.state.update(cx, |state, _| {
                state.update = Some(crate::updater::Ready::preview("0.1.3"));
            });
        }
        if args.open_account_menu {
            shell.menu.set_open(true);
        }
        if args.open_join {
            shell.toggle_join(window, cx);
        }
        if args.open_sign_in {
            shell.open_login(window, cx);
        }
        if let Some(invite) = &args.party_join
            && let Err(error) =
                shell.join_party_from_invite(invite, args.party_name.clone(), window, cx)
        {
            crate::debug::log(format!("party join refused: {error}"));
        }
        shell.state.update(cx, |state, cx| {
            if args.party_host {
                state.start_party(args.party_name.clone(), cx);
            }
            if let Some((kind, id)) = args.party_play.clone() {
                state.play_by_id(kind, id.clone(), id, Default::default());
                cx.notify();
            }
        });
        if args.party_preview.is_some() && !matches!(shell.history.current(), Route::Player { .. })
        {
            let view = crate::party_lobby::preview(args.party_preview.as_deref() == Some("host"));
            shell.state.update(cx, |state, _| {
                state.party = Some(crate::party::Party::preview(view));
            });
        }
        if let Some(host) = args.open_party_lobby {
            shell.open_party_lobby_preview(host, window, cx);
        }
        if let Some(kind) = args.open_party_prompt.as_deref()
            && let Some(film) = crate::fixtures::films().into_iter().next()
        {
            use crate::party_prompt::{Ask, Play};
            let ask = match kind {
                "stop" => Ask::StopFilm,
                "end" => Ask::EndParty,
                "switch" => Ask::JoinOther {
                    invite: String::new(),
                    hosting: true,
                },
                "next" => Ask::JoinFilm(Box::new((
                    film.clone(),
                    film.id.clone(),
                    Default::default(),
                ))),
                _ => Ask::LeaveToPlay(Play::Launch {
                    video_id: film.id.clone(),
                    film: Box::new(film),
                    resume: None,
                }),
            };
            shell.ask_party(ask, window, cx);
        }
        if args.play_hero && args.fixtures {
            cx.defer_in(window, |shell, window, cx| {
                let Some(film) = crate::fixtures::films().into_iter().next() else {
                    return;
                };
                let resume = panorama_core::stremio::ResumePoint {
                    offset_secs: 1200.0,
                    duration_secs: 6100.0,
                    video_id: None,
                };
                // What `home::play_film` does, from inside the shell.
                let id = film.id.clone();
                let video_id = resume.video_id.clone().unwrap_or_else(|| id.clone());
                shell.state.update(cx, |state, cx| {
                    state.launch(film, video_id, Some(resume), Default::default(), cx)
                });
                shell.navigate(crate::router::Route::Player { id }, window, cx);
            });
        }
        shell
    }
}

impl AppShell {
    /// Joins the party an invite link names and opens its lobby. Every way in comes through
    /// here: the sidebar's field, an opened invite link, a second launch's handoff, and a link
    /// the system opens.
    pub(crate) fn join_party_from_invite(
        &mut self,
        invite: &str,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), panorama_core::party::transport::InviteError> {
        panorama_core::party::transport::parse_invite(invite)?;
        let current = (self.state.read(cx).party.as_ref())
            .filter(|party| party.view.ended().is_none())
            .map(|party| party.view.is_host());
        if let Some(hosting) = current {
            if self.join.open {
                self.close_join(window, cx);
            }
            let invite = invite.to_owned();
            self.ask_party(
                crate::party_prompt::Ask::JoinOther { invite, hosting },
                window,
                cx,
            );
            cx.activate(true);
            window.activate_window();
            return Ok(());
        }
        self.join_party_now(invite, name, window, cx)
    }

    /// Joins `invite`, closing whatever this window was playing on its own.
    pub(crate) fn join_party_now(
        &mut self,
        invite: &str,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), panorama_core::party::transport::InviteError> {
        panorama_core::party::transport::parse_invite(invite)?;
        self.leave_party_route(window, cx);
        self.state
            .update(cx, |state, cx| state.join_party(invite, name, cx))?;
        if self.join.open {
            self.close_join(window, cx);
        }
        self.open_party_lobby_as_member(window, cx);
        cx.activate(true);
        window.activate_window();
        Ok(())
    }

    /// The sidebar's Watch party: back to the party's film with its panel open. Before the
    /// party has opened a film, the popover says so and offers a way out.
    pub(crate) fn open_party_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.state.read(cx);
        // A friend whose party moved on while they browsed goes to what it plays now.
        let moved_on = state.party.as_ref().is_some_and(|party| {
            !party.view.is_host()
                && party.media().is_some_and(|media| {
                    state
                        .playback
                        .as_ref()
                        .is_none_or(|playing| playing.video_id != media.video_id)
                })
        });
        if moved_on {
            self.follow_next_film = true;
            self.join.dismiss();
            self.state
                .update(cx, |state, cx| state.retry_party_film(cx));
            return;
        }
        let state = self.state.read(cx);
        // A host who closed the film keeps the party, with nothing to go back to: reopening
        // the last film would play it for everyone.
        let between_films = state
            .party
            .as_ref()
            .is_some_and(|party| party.view.is_host() && party.media().is_none());
        let Some(id) = (state.playback.as_ref())
            .filter(|_| !between_films)
            .map(|playing| playing.film.id.clone())
        else {
            self.toggle_join(window, cx);
            return;
        };
        self.join.dismiss();
        self.navigate(Route::Player { id }, window, cx);
        if let Some(player) = self.mounted.as_ref().and_then(|mounted| {
            mounted
                .view
                .clone()
                .downcast::<routes::player::Player>()
                .ok()
        }) {
            player.update(cx, |player, cx| player.open_party_panel(window, cx));
        }
    }

    /// Joins a party whose link a second launch handed over, as "Join watch party" does.
    fn take_handoff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self
            .state
            .read(cx)
            .services
            .as_ref()
            .filter(|services| services.store.is_some())
            .map(|services| services.data_dir.clone())
        else {
            return;
        };
        let Some(invite) = crate::party::handoff::take(&dir, std::time::SystemTime::now()) else {
            return;
        };
        if let Err(error) = self.join_party_from_invite(&invite, None, window, cx) {
            crate::debug::log(format!("party join refused: {error}"));
        }
    }

    /// Opens what the party (or `--party-play`) asks for once its details are in, and hands a
    /// debug run the host's invite.
    fn follow_party(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.invite_written
            && let Some(path) = self.args.party_invite_file.clone()
            && let Some(invite) = self
                .state
                .read(cx)
                .party
                .as_ref()
                .and_then(|party| match &party.view.role {
                    crate::party::Role::Host { invite, .. } => invite.clone(),
                    crate::party::Role::Member => None,
                })
        {
            self.invite_written =
                crate::party::handoff::write_private(&path, invite.as_str()).is_ok();
        }
        self.state.update(cx, |state, _| state.sync_party_blocked());
        if self.state.read(cx).party_stopped() {
            self.leave_party_route(window, cx);
            if self.lobby.is_none() {
                self.open_party_lobby_as_member(window, cx);
            }
        }
        let Some((film, video_id, pick)) =
            self.state.update(cx, |state, cx| state.take_ready_play(cx))
        else {
            return;
        };
        let browsing = !matches!(self.history.current(), Route::Player { .. })
            && self.lobby.is_none()
            && (self.state.read(cx).party.as_ref()).is_some_and(|party| !party.view.is_host());
        if browsing && !std::mem::take(&mut self.follow_next_film) {
            // Until they say yes, the party starts without them.
            self.state
                .read(cx)
                .party_report(panorama_core::party::MemberState::Away, 0.0, None);
            let ask = crate::party_prompt::Ask::JoinFilm(Box::new((film, video_id, pick)));
            match self.prompt.clone() {
                None => self.ask_party(ask, window, cx),
                Some(prompt) => prompt.update(cx, |prompt, cx| prompt.swap_film(ask, cx)),
            }
            return;
        }
        self.follow_next_film = false;
        let id = film.id.clone();
        if *self.history.current() == (Route::Player { id: id.clone() })
            && let Some(player) = self.mounted.as_ref().and_then(|mounted| {
                mounted
                    .view
                    .clone()
                    .downcast::<crate::routes::player::Player>()
                    .ok()
            })
        {
            player.update(cx, |player, cx| player.restart(cx));
        }
        crate::party::trace(|| format!("open film={id}"));
        self.state
            .update(cx, |state, cx| state.launch(film, video_id, None, pick, cx));
        self.navigate(Route::Player { id }, window, cx);
    }
}
