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
    AnyView, Context, Entity, FocusHandle, KeyDownEvent, NavigationDirection, Render, Window, div,
    prelude::*, px,
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
    focus: FocusHandle,
    _keystrokes: gpui::Subscription,
    titlebar: Titlebar,
    launched: Instant,
    screenshot: Option<std::path::PathBuf>,
    capture_scheduled: bool,
    player_transparent: bool,
    outcome: Sender<Result<(), String>>,
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
            focus,
            _keystrokes: keystrokes,
            titlebar: Titlebar::new(cx, state.clone()),
            launched: Instant::now(),
            screenshot: args.screenshot,
            capture_scheduled: false,
            player_transparent: false,
            outcome,
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
        if args.play_hero && args.fixtures {
            cx.defer_in(window, |shell, window, cx| {
                let Some(film) = crate::fixtures::films().into_iter().next() else {
                    return;
                };
                let resume = panorama_core::stremio::ResumePoint {
                    offset_secs: 1200.0,
                    duration_secs: 6100.0,
                };
                // What `home::play_film` does, from inside the shell.
                let id = film.id.clone();
                shell
                    .state
                    .update(cx, |state, cx| state.launch(film, Some(resume), cx));
                shell.navigate(crate::router::Route::Player { id }, window, cx);
            });
        }
        shell
    }
}
