use crate::{
    alert_row::AlertRow,
    app::AppShell,
    app_state::{Account, AppState},
    args::Args,
    assets::Weighted,
    film_grid::{FilmGrid, GridHost, reveal},
    home_layout::{Grid, ViewSettings},
    image_cache::ImageCache,
    router::Route,
    theme::Typography,
};
use futures::StreamExt;
use gpui::{
    Context, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, ScrollHandle, Subscription,
    Task, WeakEntity, Window, div, point, prelude::*, px, relative, svg,
};
use gpui_component::input::{Input, InputEvent, InputState};
use panorama_core::{
    addons::{FailureKind, FilmDetails, ResourceEvent},
    stremio::Descriptor,
};
use std::sync::Arc;

/// Space above the field, level with the Addons title.
const TOP: f32 = 56.0;
const FIELD_PAD: f32 = 12.0;
const STATUS: f32 = 16.0 + 20.0;
const GRID_GAP: f32 = 32.0;

pub struct Search {
    shell: WeakEntity<AppShell>,
    state: Entity<AppState>,
    id: u64,
    input: Entity<InputState>,
    /// Last submitted query; equals the route's query.
    query: String,
    grid: FilmGrid,
    loading: bool,
    error: Option<String>,
    request: Option<Task<()>>,
    scroll: ScrollHandle,
    images: Entity<ImageCache>,
    retry: FocusHandle,
    keyboard: bool,
    /// Inputs of the last request, so addon or account changes re-run it.
    asked: (Vec<Descriptor>, Account),
    _input: Subscription,
    _state: Subscription,
    _images: Subscription,
    _shell: Option<Subscription>,
}

impl Search {
    /// Mount with `query` in the field, searching at once when it is not empty.
    pub fn new(
        shell: WeakEntity<AppShell>,
        state: Entity<AppState>,
        args: Args,
        id: u64,
        query: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("Search films");
            input.set_value(query.clone(), window, cx);
            input
        });
        let input_events = cx.subscribe_in(&input, window, |search, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                search.submit(window, cx);
            }
        });
        let state_observer = cx.observe(&state, |search, state, cx| {
            let state = state.read(cx);
            if !search.query.is_empty()
                && (search.asked.0 != state.installed || search.asked.1 != state.account)
            {
                search.load(cx);
            }
        });
        let images = state.read(cx).images.clone();
        let image_observer = cx.observe(&images, |_, _, cx| cx.notify());
        let shell_observer = shell
            .upgrade()
            .map(|shell| cx.observe(&shell, |_, _, cx| cx.notify()));
        let mut search = Self {
            shell,
            state,
            id,
            input,
            query,
            grid: FilmGrid::new(args.hover_first_card, cx),
            loading: false,
            error: None,
            request: None,
            scroll: ScrollHandle::new(),
            images,
            retry: cx.focus_handle(),
            keyboard: args.focus_first_card,
            asked: (Vec::new(), Account::SignedOut),
            _input: input_events,
            _state: state_observer,
            _images: image_observer,
            _shell: shell_observer,
        };
        if !search.query.is_empty() {
            search.load(cx);
        }
        search.focus_field(window, cx);
        search
    }

    pub fn focus_field(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.focus_handle(cx).focus(window, cx);
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value().trim().to_string();
        if value.is_empty() {
            self.clear(window, cx);
            return;
        }
        self.record(value, cx);
        self.load(cx);
        self.scroll.set_offset(point(px(0.0), px(0.0)));
        self.focus_field(window, cx);
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.record(String::new(), cx);
        self.request = None;
        self.loading = false;
        self.error = None;
        self.grid.set_films(Arc::from([]), cx);
        self.focus_field(window, cx);
        cx.notify();
    }

    fn record(&mut self, query: String, cx: &mut Context<Self>) {
        self.query.clone_from(&query);
        let id = self.id;
        let _ = self.shell.update(cx, |shell, _| {
            shell.retitle(id, Route::Search { query });
        });
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let state = self.state.read(cx);
        self.asked = (state.installed.clone(), state.account.clone());
        let Some(services) = &state.services else {
            return;
        };
        let (job, mut receiver) = services.search(state.installed.clone(), self.query.clone());
        self.loading = true;
        self.error = None;
        self.request = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some(event) = receiver.next().await {
                let updated = entity.update(cx, |search, cx| {
                    search.loading = false;
                    match event {
                        ResourceEvent::CacheHit(films) | ResourceEvent::Fresh(films) => {
                            let films = films
                                .into_iter()
                                .filter(|film| !film.name.trim().is_empty())
                                .collect::<Vec<_>>()
                                .into();
                            search.grid.set_films(films, cx);
                        }
                        ResourceEvent::Failed(kind) => {
                            search.error = Some(
                                match kind {
                                    FailureKind::Offline => {
                                        "Can't search films. Check your connection."
                                    }
                                    FailureKind::Timeout => {
                                        "Searching films took too long. Try again."
                                    }
                                    _ => "Could not search films. Try again.",
                                }
                                .into(),
                            )
                        }
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn settings(&self, window: &Window, cx: &Context<Self>) -> ViewSettings {
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
        ViewSettings {
            width: f32::from(window.viewport_size().width),
            height: f32::from(window.viewport_size().height),
            theme,
            active,
            keyboard: keyboard || self.keyboard,
            reduced,
        }
    }

    fn field_size(width: f32) -> f32 {
        (width * 0.05).clamp(36.0, 52.0)
    }

    fn grid_top(width: f32) -> f32 {
        TOP + Self::field_size(width) * 1.2 + FIELD_PAD + 2.0 + STATUS + GRID_GAP
    }

    fn status(&self) -> String {
        if self.query.is_empty() {
            "Type a title and press Enter.".into()
        } else if self.loading {
            format!("Searching for \u{201c}{}\u{201d}", self.query)
        } else if self.error.is_some() {
            String::new()
        } else {
            match self.grid.films.len() {
                0 => format!("No films found for \u{201c}{}\u{201d}.", self.query),
                1 => format!("1 film for \u{201c}{}\u{201d}", self.query),
                count => format!("{count} films for \u{201c}{}\u{201d}", self.query),
            }
        }
    }
}

impl GridHost for Search {
    fn film_grid(&mut self) -> &mut FilmGrid {
        &mut self.grid
    }
    fn focus_card(
        &mut self,
        index: usize,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.settings(window, cx);
        let col = view.column();
        let last = self.grid.len().saturating_sub(1);
        if !forward && index == 0 {
            self.scroll.set_offset(point(px(0.0), px(0.0)));
            self.focus_field(window, cx);
        } else if forward && index >= last {
            self.scroll.set_offset(point(px(0.0), px(0.0)));
            let _ = self
                .shell
                .update(cx, |shell, cx| shell.focus_sidebar(window, cx));
        } else {
            let next = if forward { index + 1 } else { index - 1 };
            let y = Self::grid_top(view.width) + FilmGrid::row_top(next, col);
            reveal(&self.scroll, y, Grid::fit(col.width).height(), view.height);
            if let Some(card) = self.grid.focus(next) {
                card.focus(window, cx);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn open_film(&mut self, film: FilmDetails, window: &mut Window, cx: &mut Context<Self>) {
        super::home::open_film(&self.state, &self.shell, film, window, cx);
    }
}

impl Render for Search {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.settings(window, cx);
        let theme = view.theme;
        let col = view.column();
        if view.active {
            self.images.update(cx, |images, _| images.begin_frame());
        }
        let focused = self.input.focus_handle(cx).is_focused(window);
        let size = Self::field_size(view.width);
        let field = div()
            .w_full()
            .pb(px(FIELD_PAD + if focused { 0.0 } else { 1.0 }))
            .border_b(px(if focused { 2.0 } else { 1.0 }))
            .border_color(if focused { theme.ink } else { theme.ink_muted })
            .flex()
            .items_center()
            .gap(px(16.0))
            .child(
                svg()
                    .path("search.svg")
                    .size(px(size * 0.6))
                    .flex_shrink_0()
                    .text_color(if focused { theme.ink } else { theme.ink_muted }),
            )
            .child(
                Input::new(&self.input)
                    .appearance(false)
                    .bordered(false)
                    .focus_bordered(false)
                    .aria_label("Search films")
                    .tab_index(if view.active { 0 } else { -1 })
                    .disabled(!view.active)
                    .map(|input| gpui::Styled::h(input, px(size * 1.2)))
                    .flex_1()
                    .p_0()
                    .text_size(px(size))
                    .weight(FontWeight::MEDIUM)
                    .line_height(relative(1.2))
                    .text_color(theme.ink),
            );
        let status = self.status();
        let empty = !self.query.is_empty()
            && !self.loading
            && self.error.is_none()
            && self.grid.films.is_empty();
        // The alert row carries the message; the line above it stays quiet.
        let line = if empty { String::new() } else { status.clone() };
        let mut section = div()
            .ml(px(col.left))
            .w(px(col.width))
            .pt(px(TOP))
            .pb(px(112.0))
            .child(field)
            .child(
                div()
                    .mt(px(16.0))
                    .h(px(20.0))
                    .label()
                    .text_color(theme.ink_muted)
                    .child(line),
            );
        if let Some(message) = self.error.clone() {
            section = section.child(self.alert(message, "Try again", view, cx));
        } else if empty {
            section = section.child(self.alert(
                format!("No films found for \u{201c}{}\u{201d}.", self.query),
                "Clear search",
                view,
                cx,
            ));
        } else if !self.query.is_empty() {
            let top = -f32::from(self.scroll.offset().y);
            let offset = Self::grid_top(view.width) - top;
            let services = self.state.read(cx).services.clone();
            let images = self.images.clone();
            section = section.child(
                self.grid
                    .render_rows(
                        offset,
                        self.loading,
                        view,
                        &images,
                        services.as_ref(),
                        window,
                        cx,
                    )
                    .mt(px(GRID_GAP)),
            );
        }
        if view.active {
            self.images.update(cx, |images, _| images.finish_frame());
        }
        if self.loading
            || self.grid.moving(view.reduced)
            || (!view.reduced && view.active && self.images.read(cx).moving())
        {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        div()
            .id(("search-scroll", self.id))
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
            .capture_key_down(cx.listener(|search, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape"
                    && (!search.query.is_empty() || !search.input.read(cx).value().is_empty())
                {
                    search.clear(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(section)
            .child(crate::a11y::status("search-status", status))
    }
}

impl Search {
    fn alert(
        &self,
        message: String,
        label: &'static str,
        view: ViewSettings,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        AlertRow {
            message,
            label,
            theme: view.theme,
            width: view.width,
            focus: &self.retry,
            active: view.active,
            keyboard: view.keyboard,
        }
        .render(cx.listener(move |search, _, window, cx| {
            if label == "Clear search" {
                search.clear(window, cx);
            } else {
                search.load(cx);
            }
        }))
    }
}
