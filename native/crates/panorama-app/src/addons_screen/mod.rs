//! The Addons screen: install from a link, reorder and remove, and pick the film details source.
mod actions;
mod backend;
mod choices;
pub mod fixture;
mod install_view;
mod installed_view;
pub mod model;
#[cfg(test)]
mod model_tests;
mod source_view;
mod state;
#[cfg(test)]
mod state_tests;
mod widgets;

use crate::{
    alert_row::AlertRow,
    app::AppShell,
    app_state::{Account, AppState},
    args::Args,
    footer::footer,
    image_cache::ImageCache,
    theme::{Typography, content_width},
    transition::Tween,
};
use backend::{Backend, Token};
use gpui::{
    AppContext, Context, Entity, FocusHandle, Render, ScrollHandle, Subscription, Window, div,
    prelude::*, px,
};
use gpui_component::input::{InputEvent, InputState};
use model::{InstallFlow, InstalledRow, PreviewCard, Screen, screen};
use panorama_core::addons::AddonKey;
use state::{Latest, MetaState};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Instant,
};
use widgets::View;

/// One focusable control, keyed so handles survive reordering.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum Slot {
    Preview,
    Install,
    Cancel,
    Retry,
    SignIn,
    Up(AddonKey),
    Down(AddonKey),
    Configure(AddonKey),
    Remove(AddonKey),
    ConfirmRemove(AddonKey),
    Keep(AddonKey),
    Radio(Option<AddonKey>),
}

/// Retained Addons entry with its own scroll, focus and in-flight work.
pub struct Addons {
    shell: gpui::WeakEntity<AppShell>,
    state: Entity<AppState>,
    args: Args,
    id: u64,
    scroll: ScrollHandle,
    backend: Option<Backend>,
    images: Entity<ImageCache>,
    input: Entity<InputState>,
    flow: InstallFlow<Token>,
    shown: Option<PreviewCard>,
    reveal: Tween,
    measured: Rc<Cell<f32>>,
    press: Tween,
    loading: bool,
    load_failed: bool,
    seeded: bool,
    rows: Vec<InstalledRow>,
    meta: MetaState,
    previews: Latest,
    focus_after: Option<AddonKey>,
    dots: HashMap<Option<AddonKey>, Tween>,
    slide: HashMap<AddonKey, Tween>,
    present: HashMap<AddonKey, Tween>,
    leaving: HashSet<AddonKey>,
    notice: Option<String>,
    pending_input: Option<String>,
    notice_generation: u64,
    handles: HashMap<Slot, FocusHandle>,
    launched: Instant,
    signed_in: bool,
    _subscriptions: [Subscription; 3],
}

fn signed_in(state: &AppState) -> bool {
    matches!(state.account, Account::SignedIn(_))
}

impl Addons {
    /// Create one history entry's view and start reading the installed list.
    pub fn new(
        shell: gpui::WeakEntity<AppShell>,
        state: Entity<AppState>,
        args: Args,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Paste an addon link"));
        let input_events =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => this.input_changed(cx),
                _ => {}
            });
        let account = cx.observe(&state, |this, state, cx| {
            let now = signed_in(state.read(cx));
            if now != this.signed_in {
                this.signed_in = now;
                this.rows.clear();
                this.leaving.clear();
                this.flow = InstallFlow::Idle;
                this.previews.next();
                this.input
                    .update(cx, |input, cx| input.set_readonly(false, cx));
                if now {
                    this.reload(cx);
                }
            }
            cx.notify();
        });
        let shell_observer = match shell.upgrade() {
            Some(shell) => cx.observe(&shell, |_, _, cx| cx.notify()),
            None => cx.on_release(|_, _| {}),
        };
        let current = state.read(cx);
        let backend = current
            .services
            .as_ref()
            .map(|services| Backend::new(services.session.clone(), services.runtime.clone()));
        let images = current.images.clone();
        let is_signed_in = signed_in(current);
        let mut addons = Self {
            shell,
            state,
            args,
            id,
            scroll: ScrollHandle::new(),
            backend,
            images,
            input,
            flow: InstallFlow::Idle,
            shown: None,
            reveal: Tween::fixed(0.0),
            measured: Rc::new(Cell::new(0.0)),
            press: Tween::fixed(1.0),
            loading: false,
            load_failed: false,
            seeded: false,
            rows: vec![],
            meta: MetaState::default(),
            previews: Latest::default(),
            focus_after: None,
            dots: HashMap::new(),
            slide: HashMap::new(),
            present: HashMap::new(),
            leaving: HashSet::new(),
            notice: None,
            pending_input: None,
            notice_generation: 0,
            handles: HashMap::new(),
            launched: Instant::now(),
            signed_in: is_signed_in,
            _subscriptions: [input_events, account, shell_observer],
        };
        if is_signed_in {
            addons.reload(cx);
        }
        addons
    }

    fn handle(&mut self, slot: Slot, cx: &mut Context<Self>) -> FocusHandle {
        self.handles
            .entry(slot)
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    fn view(&self, window: &Window, cx: &Context<Self>) -> View {
        let (theme, active, keyboard, reduced) = self
            .shell
            .upgrade()
            .map(|shell| {
                let shell = shell.read(cx);
                (
                    shell.theme,
                    shell.is_current(self.id),
                    shell.keyboard_navigation,
                    shell.reduced_motion,
                )
            })
            .unwrap_or_default();
        View {
            theme,
            active,
            keyboard,
            reduced,
            width: f32::from(window.viewport_size().width),
            spin: self.launched.elapsed().as_secs_f32() * std::f32::consts::TAU,
        }
    }

    fn animating(&self, now: Instant, reduced: bool) -> bool {
        self.reveal.moving(now, reduced)
            || self.press.moving(now, reduced)
            || self.flow.busy()
            || self.loading
            || self.rows.iter().any(|row| {
                matches!(
                    row.pending,
                    model::RowPending::Removing | model::RowPending::Moving
                )
            })
            || self.slide.values().any(|t| t.moving(now, reduced))
            || self.present.values().any(|t| t.moving(now, reduced))
            || self.dots.values().any(|t| t.moving(now, reduced))
    }

    fn settle(&mut self, now: Instant, reduced: bool) {
        self.flow.expire(now);
        let gone: Vec<_> = self
            .leaving
            .iter()
            .filter(|key| {
                self.present
                    .get(*key)
                    .is_none_or(|t| t.value(now, reduced) <= 0.0 && !t.moving(now, reduced))
            })
            .cloned()
            .collect();
        for key in &gone {
            self.present.remove(key);
        }
        state::finish_leaving(&mut self.rows, &mut self.leaving, &gone);
        self.slide.retain(|_, t| t.moving(now, reduced));
    }
}

impl Render for Addons {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view(window, cx);
        let theme = view.theme;
        let now = Instant::now();
        self.settle(now, view.reduced);
        if let Some(key) = self.focus_after.take() {
            self.handle(Slot::Remove(key), cx).focus(window, cx);
        }
        if let Some(value) = self.pending_input.take() {
            self.input.update(cx, |input, cx| {
                input.set_readonly(false, cx);
                input.set_value(value, window, cx);
            });
        }
        let (is_signed_in, runtime_error) = {
            let state = self.state.read(cx);
            (signed_in(state), state.runtime_error.clone())
        };
        let current = screen(is_signed_in, self.loading, self.load_failed);
        let container = content_width(view.width);
        let mut page = div()
            .w(px(container))
            .mx_auto()
            .pt(px(56.0))
            .pb(px(112.0))
            .child(
                div()
                    .id("addons-title")
                    .display(view.width)
                    .text_color(theme.ink)
                    .role(gpui::Role::Heading)
                    .child("Addons"),
            )
            .child(
                div()
                    .body()
                    .text_color(theme.ink_muted)
                    .mt(px(12.0))
                    .max_w(px(560.0))
                    .child(
                        "Addons supply catalogs, film details and streams. \
                         Panorama asks them in the order below.",
                    ),
            );
        if let Some(message) = runtime_error {
            let focus = self.handle(Slot::Retry, cx);
            page = page.child(
                AlertRow {
                    message,
                    label: "Reload Panorama",
                    theme,
                    width: view.width,
                    focus: &focus,
                    active: view.active,
                    keyboard: view.keyboard,
                }
                .render(
                    window,
                    cx.listener(|this, _, _, cx| {
                        this.state.update(cx, |state, cx| state.reload(cx))
                    }),
                ),
            );
        } else if current == Screen::SignedOut {
            let focus = self.handle(Slot::SignIn, cx);
            page = page.child(
                div().mt(px(24.0)).child(
                    AlertRow {
                        message: "Sign in to manage addons.".into(),
                        label: "Sign in",
                        theme,
                        width: view.width,
                        focus: &focus,
                        active: view.active,
                        keyboard: view.keyboard,
                    }
                    .render(
                        window,
                        cx.listener(|this, _, window, cx| {
                            let _ = this
                                .shell
                                .update(cx, |shell, cx| shell.open_login(window, cx));
                        }),
                    ),
                ),
            );
        } else {
            self.images.update(cx, |images, _| images.begin_frame());
            page = page
                .child(self.install_section(view, window, cx))
                .child(self.installed_section(current, view, window, cx));
            if current == Screen::Ready {
                page = page.child(self.source_section(view, window, cx));
            }
            self.images.update(cx, |images, _| images.finish_frame());
        }
        if self.animating(now, view.reduced) {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        div()
            .id("addons-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(page)
            .child(footer(theme, view.width))
    }
}
