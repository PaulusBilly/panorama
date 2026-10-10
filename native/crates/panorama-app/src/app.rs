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
        shell.present(cx);
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

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.viewport != window.viewport_size() {
            self.viewport = window.viewport_size();
            self.menu.dismiss();
        }
        self.presence
            .retain(|presence| !presence.exiting || !presence.settled(now));
        let moving = !self.reduced_motion
            && (self.presence.iter().any(|presence| !presence.settled(now))
                || self.header_mix.moving(now, false)
                || self.logo_mix.moving(now, false)
                || self.header_slide.moving(now, false)
                || self.trigger.moving(now, false)
                || self.menu.moving(false));
        let capture_waiting = self.screenshot.is_some() && !self.capture_scheduled;
        if moving || capture_waiting {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        if capture_waiting
            && !moving
            && now.duration_since(self.launched) >= Duration::from_millis(1600)
            && let Some(path) = self.screenshot.take()
        {
            self.capture_scheduled = true;
            debug::screenshot::schedule(window, cx, path, self.outcome.clone());
        }
        let mut body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden();
        for presence in &self.presence {
            let pose = if self.reduced_motion {
                Pose {
                    opacity: 1.0,
                    y: 0.0,
                }
            } else {
                presence.pose(now)
            };
            let mounted = &presence.view;
            let mut layer = div()
                .id(("route-presence", mounted.id))
                .absolute()
                .inset_0()
                .top(px(pose.y))
                .bottom(px(-pose.y))
                .opacity(pose.opacity)
                .flex()
                .flex_col();
            layer = layer.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when(
                        !matches!(mounted.route, Route::Home | Route::Player { .. }),
                        |view| view.pt(px(60.0)),
                    )
                    .child(mounted.view.clone()),
            );
            if !matches!(mounted.route, Route::Player { .. }) {
                let home = mounted.route == Route::Home;
                layer = layer.child(mounted.header.render(
                    &mounted.route,
                    HeaderAppearance {
                        theme: self.theme,
                        active: !presence.exiting
                            && self.login.is_none()
                            && (!home || !self.sticky.hidden),
                        keyboard: self.keyboard_navigation,
                        foreground: if home {
                            self.header_foreground()
                        } else {
                            self.theme.ink
                        },
                        logo: if home {
                            gpui::Rgba {
                                r: self.logo_mix.value(now, self.reduced_motion),
                                g: self.logo_mix.value(now, self.reduced_motion),
                                b: self.logo_mix.value(now, self.reduced_motion),
                                a: 1.0,
                            }
                        } else {
                            gpui::black().into()
                        },
                        background: if home {
                            self.header_background()
                        } else {
                            self.theme.canvas
                        },
                        y: if home { self.header_y() } else { 0.0 },
                        pressed: self.trigger_scale(),
                    },
                    window,
                    cx,
                ));
            }
            if presence.exiting {
                layer = layer.child(div().absolute().inset_0().occlude());
            }
            body = body.child(layer);
        }
        if self.menu.visible(self.reduced_motion) {
            body = body.child(self.menu.render(
                crate::home_layout::ViewSettings {
                    theme: self.theme,
                    keyboard: self.keyboard_navigation,
                    reduced: self.reduced_motion,
                    active: true,
                    width: f32::from(window.viewport_size().width),
                    height: f32::from(window.viewport_size().height),
                },
                matches!(self.state.read(cx).account, Account::SignedIn(_)),
                window,
                cx,
            ));
        }
        div()
            .id("panorama-shell")
            .size_full()
            .flex()
            .flex_col()
            .bg(self.theme.canvas)
            .text_color(self.theme.ink)
            .font(assets::font())
            .font_features(gpui::FontFeatures(std::sync::Arc::new(vec![(
                "kern".into(),
                1,
            )])))
            .when(capture_waiting, |shell| {
                let outcome = self.outcome.clone();
                shell.child(
                    gpui::canvas(
                        move |_, window, cx| {
                            if let Err(error) = debug::check_default_font(window) {
                                let _ = outcome.send(Err(error));
                                cx.quit();
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size(px(0.0)),
                )
            })
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|shell, _, _, cx| {
                shell.keyboard_navigation = true;
                cx.notify();
            }))
            .on_key_down(cx.listener(Self::key_down))
            .on_scroll_wheel(cx.listener(|shell, _, _, cx| {
                if shell.menu.open {
                    shell.menu.dismiss();
                    cx.notify();
                }
            }))
            .on_any_mouse_down(
                cx.listener(|shell, event: &gpui::MouseDownEvent, window, cx| {
                    shell.keyboard_navigation = false;
                    if shell.menu.open {
                        let width = f32::from(window.viewport_size().width);
                        let inset = (width - crate::theme::content_width(width)) * 0.5;
                        let x = f32::from(event.position.x);
                        let y = f32::from(event.position.y)
                            - if window.is_fullscreen() { 0.0 } else { 32.0 };
                        let in_popup = x >= width - inset - 192.0
                            && x <= width - inset
                            && (68.0..=132.0).contains(&y);
                        let in_trigger = x >= width - inset - 40.0
                            && x <= width - inset
                            && (20.0..=60.0).contains(&y);
                        if !in_popup && !in_trigger {
                            shell.menu.set_open(false);
                        }
                    }
                    cx.notify();
                    match event.button {
                        gpui::MouseButton::Navigate(NavigationDirection::Back) => {
                            shell.back(window, cx)
                        }
                        gpui::MouseButton::Navigate(NavigationDirection::Forward) => {
                            shell.forward(window, cx)
                        }
                        _ => return,
                    }
                    cx.stop_propagation();
                }),
            )
            .when(!window.is_fullscreen(), |shell| {
                shell.child(
                    self.titlebar
                        .render(self.theme, self.keyboard_navigation, window),
                )
            })
            .child(body)
            .when_some(self.login.clone(), |shell, login| shell.child(login))
    }
}
