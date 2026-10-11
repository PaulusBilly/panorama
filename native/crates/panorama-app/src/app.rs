//! The retained window shell and route presentation.
use crate::{
    account_menu::AccountMenu,
    app_state::{Account, AppState},
    args::Args,
    assets, debug,
    header::{Header, HeaderAppearance},
    header_state::{HeaderColor, Sticky},
    login::LoginDialog,
    motion::{Pose, Presence},
    router::{History, Route},
    routes,
    search_band::SearchBand,
    theme::Theme,
    theme::motion::{EASE_IN_OUT, EASE_OUT, HEADER_COLOR, HEADER_SLIDE},
    titlebar::Titlebar,
    transition::Tween,
};
use gpui::{
    AnyView, Context, Entity, FocusHandle, KeyDownEvent, NavigationDirection, Render, Window, div,
    prelude::*, px,
};
use gpui_component::WindowExt;
use gpui_component::input::{InputEvent, InputState};
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
#[path = "app_search.rs"]
mod search;
#[path = "app_view.rs"]
mod view;

struct Mounted {
    id: u64,
    route: Route,
    header: Header,
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
    /// Home's scroll-driven header policy.
    pub sticky: Sticky,
    pub(crate) search: SearchBand,
    search_input: Entity<InputState>,
    search_submit: FocusHandle,
    search_disclosures: std::collections::HashMap<u64, bool>,
    _search_input: gpui::Subscription,
    login: Option<Entity<LoginDialog>>,
    args: Args,
    header_color: HeaderColor,
    header_from: (gpui::Rgba, gpui::Rgba),
    header_mix: Tween,
    logo_mix: Tween,
    header_slide: Tween,
    trigger: Tween,
    viewport: gpui::Size<gpui::Pixels>,
    _state: gpui::Subscription,
    history: History,
    cache: VecDeque<Rc<Mounted>>,
    presence: Vec<Presence<Rc<Mounted>>>,
    focus: FocusHandle,
    _keystrokes: gpui::Subscription,
    titlebar: Titlebar,
    launched: Instant,
    screenshot: Option<std::path::PathBuf>,
    capture_scheduled: bool,
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
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let search_observer =
            cx.subscribe_in(&search_input, window, |shell, input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    shell.submit_search(window, cx);
                } else if matches!(event, InputEvent::Change) {
                    shell.search.query = input.read(cx).value().to_string();
                }
            });
        let theme = if args.dark {
            Theme::dark()
        } else {
            Theme::light()
        };
        let mut shell = Self {
            theme: if args.dark {
                Theme::dark()
            } else {
                Theme::light()
            },
            reduced_motion: args.reduced_motion,
            keyboard_navigation: false,
            state,
            menu: AccountMenu::new(cx),
            sticky: Sticky::home(),
            search: SearchBand::new(&args.route, args.open_search),
            search_input,
            search_submit: cx.focus_handle(),
            search_disclosures: std::collections::HashMap::new(),
            _search_input: search_observer,
            login: None,
            args: args.clone(),
            header_color: HeaderColor::HeroLoading,
            header_from: (theme.ink, theme.canvas.opacity(0.0)),
            header_mix: Tween::fixed(1.0),
            logo_mix: Tween::fixed(0.0),
            header_slide: Tween::fixed(0.0),
            trigger: Tween::fixed(1.0),
            viewport: window.viewport_size(),
            _state: state_observer,
            history: History::new(args.route),
            cache: VecDeque::new(),
            presence: Vec::new(),
            focus,
            _keystrokes: keystrokes,
            titlebar: Titlebar::new(cx),
            launched: Instant::now(),
            screenshot: args.screenshot,
            capture_scheduled: false,
            outcome,
        };
        shell
            .search_disclosures
            .insert(shell.history.current_id(), shell.search.open);
        shell.present(cx);
        if args.signed_out && args.fixtures {
            shell.state.update(cx, |state, cx| {
                state.account = Account::SignedOut;
                cx.notify();
            });
        }
        if shell.search.open {
            shell.search_input.update(cx, |input, cx| {
                input.set_value(shell.search.query.clone(), window, cx)
            });
            gpui::Focusable::focus_handle(&shell.search_input, cx).focus(window, cx);
            shell.search_caret(cx);
        }
        if args.open_account_menu {
            shell.menu.set_open(true);
        }
        if args.open_sign_in {
            shell.open_login(window, cx);
        }
        shell
    }
}

fn mix(from: gpui::Rgba, to: gpui::Rgba, t: f32) -> gpui::Rgba {
    gpui::Rgba {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}
