//! The retained window shell and route presentation.
use crate::{
    account_menu::AccountMenu,
    app_state::{Account, AppState},
    args::Args,
    assets, debug,
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
    pub(crate) sidebar: Sidebar,
    login: Option<Entity<LoginDialog>>,
    args: Args,
    viewport: gpui::Size<gpui::Pixels>,
    _state: gpui::Subscription,
    history: History,
    cache: VecDeque<Rc<Mounted>>,
    mounted: Option<Rc<Mounted>>,
    /// A card growing into its film page, over the route that is still mounted.
    zoom: Option<crate::zoom::Zoom>,
    /// The card the open film page grew from.
    returning: Option<crate::zoom::Returning>,
    focus: FocusHandle,
    _keystrokes: gpui::Subscription,
    titlebar: Titlebar,
    launched: Instant,
    screenshot: Option<std::path::PathBuf>,
    capture_scheduled: bool,
    player_transparent: bool,
    outcome: Sender<Result<(), String>>,
    _party: gpui::Subscription,
    invite_written: bool,
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
        let theme = Theme::default();
        let mut shell = Self {
            theme,
            reduced_motion: args.reduced_motion,
            keyboard_navigation: false,
            state: state.clone(),
            menu: AccountMenu::new(cx),
            sidebar: Sidebar::new(cx),
            login: None,
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
        if args.open_account_menu {
            shell.menu.set_open(true);
        }
        if args.open_sign_in {
            shell.open_login(window, cx);
        }
        shell.state.update(cx, |state, cx| {
            if args.party_host {
                state.start_party(args.party_name.clone(), cx);
            }
            if let Some(invite) = &args.party_join
                && let Err(error) = state.join_party(invite, args.party_name.clone(), cx)
            {
                crate::debug::log(format!("party join refused: {error}"));
            }
            if let Some((kind, id)) = args.party_play.clone() {
                state.play_by_id(kind, id.clone(), id, Default::default());
                cx.notify();
            }
        });
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
            self.invite_written = std::fs::write(path, invite.as_str()).is_ok();
        }
        let Some((film, video_id, pick)) =
            self.state.update(cx, |state, cx| state.take_ready_play(cx))
        else {
            return;
        };
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
