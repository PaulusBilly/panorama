//! The retained window shell and route presentation.
use crate::{
    args::Args,
    assets, debug,
    header::Header,
    motion::{Pose, Presence},
    router::{History, Route},
    routes,
    theme::Theme,
    titlebar::Titlebar,
};
use gpui::{
    AnyView, Context, FocusHandle, KeyDownEvent, NavigationDirection, Render, Window, div,
    prelude::*, px,
};
use gpui_component::WindowExt;
use std::{
    collections::VecDeque,
    rc::Rc,
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

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
        let mut shell = Self {
            theme: if args.dark {
                Theme::dark()
            } else {
                Theme::light()
            },
            reduced_motion: args.reduced_motion,
            keyboard_navigation: false,
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
        shell
    }

    /// Push a route while preserving existing view instances for backward navigation.
    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.push(route) {
            self.changed(window, cx);
        }
    }

    /// Go backward when history permits it.
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.can_back() && self.history.back() {
            self.changed(window, cx);
        }
    }

    /// Go forward when history permits it.
    pub fn forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.can_forward() && self.history.forward() {
            self.changed(window, cx);
        }
    }

    /// Whether an entry owns the active route controls.
    pub fn is_current(&self, id: u64) -> bool {
        self.history.current_id() == id
    }

    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.present(cx);
        cx.notify();
    }

    fn present(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let id = self.history.current_id();
        let existing = self
            .cache
            .iter()
            .position(|entry| entry.id == id)
            .and_then(|index| self.cache.remove(index))
            .or_else(|| {
                self.presence
                    .iter()
                    .find(|presence| presence.view.id == id)
                    .map(|presence| presence.view.clone())
            });
        let mounted = existing.unwrap_or_else(|| {
            let route = self.history.current().clone();
            Rc::new(Mounted {
                id,
                header: Header::new(cx),
                view: routes::create(&route, id, cx.weak_entity(), cx),
                route,
            })
        });
        self.cache.retain(|entry| self.history.contains(entry.id));
        self.cache.push_back(mounted.clone());
        while self.cache.len() > 8 {
            self.cache.pop_front();
        }
        if self.reduced_motion {
            self.presence.clear();
        }
        for presence in &mut self.presence {
            if presence.view.id == id {
                presence.resume(now);
            } else {
                presence.exit(now);
            }
        }
        if !self.presence.iter().any(|presence| presence.view.id == id) {
            self.presence.push(Presence::enter(mounted, now));
        }
        self.presence.sort_by_key(|presence| !presence.exiting);
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = &event.keystroke.modifiers;
        let input = window.has_focused_input(cx);
        match key {
            "left" if modifiers.alt => self.back(window, cx),
            "right" if modifiers.alt => self.forward(window, cx),
            "backspace"
                if !input && !modifiers.control && !modifiers.alt && !modifiers.platform =>
            {
                self.back(window, cx)
            }
            "k" if modifiers.control => self.navigate(
                Route::Search {
                    query: String::new(),
                },
                window,
                cx,
            ),
            "/" if !input && !modifiers.control && !modifiers.alt && !modifiers.platform => self
                .navigate(
                    Route::Search {
                        query: String::new(),
                    },
                    window,
                    cx,
                ),
            "escape" if matches!(self.history.current(), Route::Player { .. }) => {
                self.back(window, cx)
            }
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.presence
            .retain(|presence| !presence.exiting || !presence.settled(now));
        let moving =
            !self.reduced_motion && self.presence.iter().any(|presence| !presence.settled(now));
        let capture_waiting = self.screenshot.is_some() && !self.capture_scheduled;
        if moving || capture_waiting {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        if capture_waiting
            && !moving
            && now.duration_since(self.launched) >= Duration::from_millis(500)
            && let Some(path) = self.screenshot.take()
        {
            self.capture_scheduled = true;
            debug::screenshot::schedule(window, cx, path, self.outcome.clone());
        }
        let mut body = div().relative().flex_1().min_h_0().w_full();
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
            if !matches!(mounted.route, Route::Player { .. }) {
                layer = layer.child(mounted.header.render(
                    &mounted.route,
                    !presence.exiting,
                    self.theme,
                    self.keyboard_navigation,
                    window,
                    cx,
                ));
            }
            layer = layer.child(div().flex_1().min_h_0().child(mounted.view.clone()));
            if presence.exiting {
                layer = layer.child(div().absolute().inset_0().occlude());
            }
            body = body.child(layer);
        }
        div()
            .id("panorama-shell")
            .size_full()
            .flex()
            .flex_col()
            .bg(self.theme.canvas)
            .text_color(self.theme.ink)
            .font(assets::font())
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
            .on_any_mouse_down(
                cx.listener(|shell, event: &gpui::MouseDownEvent, window, cx| {
                    shell.keyboard_navigation = false;
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
    }
}
