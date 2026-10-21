pub(crate) use crate::film_grid::{NextCard, PreviousCard};
use crate::{
    app::AppShell,
    app_state::AppState,
    args::Args,
    film_grid::{FilmGrid, GridHost},
    home_layout::ViewSettings,
    image_cache::{CachedImage, ImageCache},
    router::Route,
    theme::motion::*,
    transition::Tween,
};
use gpui::{
    Context, Entity, FocusHandle, ScrollHandle, Subscription, WeakEntity, Window, point,
    prelude::*, px,
};
use panorama_core::addons::FilmDetails;
use std::{sync::Arc, time::Instant};

#[path = "home_alert.rs"]
mod alert;
#[path = "home_catalog.rs"]
mod catalog;
#[path = "home_keyboard.rs"]
mod keyboard;
#[path = "home_view.rs"]
mod view;

/// Retained Home catalog, virtual row state and visible image interests.
pub struct Home {
    pub(crate) shell: WeakEntity<AppShell>,
    pub(crate) state: Entity<AppState>,
    args: Args,
    id: u64,
    scroll: ScrollHandle,
    pub(crate) grid: FilmGrid,
    watch: FocusHandle,
    switcher: crate::catalog_switcher::CatalogSwitcher,
    retry: FocusHandle,
    reload: FocusHandle,
    more: FocusHandle,
    images: Entity<ImageCache>,
    surface: Tween,
    ready: bool,
    last_top: f32,
    launched: Instant,
    debug_step: usize,
    debug_focus: bool,
    benchmark: Option<crate::debug::performance::Samples>,
    _watch_focus: Option<Subscription>,
    _images: Subscription,
    _catalog: Subscription,
    _shell: Subscription,
}
impl Home {
    /// Create one history entry with independent scroll, focus and request lifetimes.
    pub fn new(
        shell: WeakEntity<AppShell>,
        state: Entity<AppState>,
        args: Args,
        id: u64,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut films: Arc<[FilmDetails]> = state
            .read(cx)
            .catalog
            .as_ref()
            .map(|p| p.films.clone())
            .unwrap_or_default()
            .into();
        if args.bench_scroll {
            films = fixture_benchmark().into();
        }
        let observer = cx.observe(&state, |home, state, cx| {
            if !home.args.bench_scroll {
                let films = state
                    .read(cx)
                    .catalog
                    .as_ref()
                    .map(|p| p.films.clone())
                    .unwrap_or_default()
                    .into();
                home.grid.set_films(films, cx);
            }
            cx.notify();
        });
        let shell_observer = if let Some(shell) = shell.upgrade() {
            cx.observe(&shell, |_, _, cx| cx.notify())
        } else {
            cx.on_release(|_, _| {})
        };
        let images = state.read(cx).images.clone();
        let image_observer = cx.observe(&images, |_, _, cx| cx.notify());
        let watch = cx.focus_handle();
        let mut switcher =
            crate::catalog_switcher::CatalogSwitcher::new(cx.focus_handle(), cx.focus_handle());
        switcher.set_open(args.open_catalog_menu);
        let mut grid = FilmGrid::new(args.hover_first_card, cx);
        grid.set_films(films, cx);
        Self {
            shell,
            state,
            args,
            id,
            scroll: ScrollHandle::new(),
            grid,
            watch,
            switcher,
            retry: cx.focus_handle(),
            reload: cx.focus_handle(),
            more: cx.focus_handle(),
            images,
            surface: Tween::fixed(1.0),
            ready: false,
            last_top: 0.0,
            launched: Instant::now(),
            debug_step: 0,
            debug_focus: false,
            benchmark: None,
            _watch_focus: None,
            _images: image_observer,
            _catalog: observer,
            _shell: shell_observer,
        }
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
            // The titlebar overlays the page on every platform.
            height: f32::from(window.viewport_size().height),
            theme,
            active,
            keyboard: keyboard || self.args.focus_first_card,
            reduced,
        }
    }
    fn artwork(
        &mut self,
        film: &FilmDetails,
        bounds: (f32, f32),
        visible: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Option<CachedImage>, bool, bool) {
        let services = self.state.read(cx).services.clone();
        crate::film_grid::artwork(
            &self.images,
            services.as_ref(),
            film,
            bounds,
            visible,
            window,
            cx,
        )
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = self.settings(window, cx);
        let now = Instant::now();
        if view.active
            && self.debug_step < self.args.scroll.len()
            && now.duration_since(self.launched).as_secs_f32() > 0.25 + 0.2 * self.debug_step as f32
        {
            let top = -f32::from(self.scroll.offset().y);
            let max = f32::from(self.scroll.max_offset().y);
            let top = (top + self.args.scroll[self.debug_step]).min(max).max(0.0);
            self.scroll.set_offset(point(px(0.0), px(-top)));
            self.debug_step += 1;
            cx.notify();
        }
        if view.active
            && self.args.focus_first_card
            && !self.debug_focus
            && let Some(first) = self.grid.focus(0)
            && self.debug_step == self.args.scroll.len()
        {
            first.focus(window, cx);
            self.debug_focus = true;
            let _ = self.shell.update(cx, |shell, cx| {
                shell.keyboard_navigation = true;
                cx.notify();
            });
            cx.notify();
        }
        let top = -f32::from(self.scroll.offset().y);
        if top != self.last_top {
            self.last_top = top;
            cx.notify();
        }
        if view.active
            && self.args.bench_scroll
            && now.duration_since(self.launched).as_secs_f32() > 1.0
        {
            let samples = self
                .benchmark
                .get_or_insert_with(|| crate::debug::performance::Samples::new(now));
            samples.frame(now);
            let elapsed = now.duration_since(samples.start).as_secs_f32();
            let max = f32::from(self.scroll.max_offset().y);
            let fraction = if elapsed < 2.5 {
                elapsed / 2.5
            } else {
                (5.0 - elapsed) / 2.5
            }
            .clamp(0.0, 1.0);
            self.scroll.set_offset(point(px(0.0), px(-max * fraction)));
            cx.notify();
            if elapsed >= 5.0 {
                let report = samples.report(now);
                crate::debug::log(report);
                cx.quit();
            }
        }
    }
}

impl GridHost for Home {
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
        self.step_card(index, forward, window, cx);
    }
    /// Open the card's opaque film identifier in the existing router.
    fn open_film(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        open_film(&self.state, &self.shell, &self.grid, id, window, cx);
    }
}

/// Hand the card's film to Film as its preview, then navigate there.
pub(crate) fn open_film<V: 'static>(
    state: &Entity<AppState>,
    shell: &WeakEntity<AppShell>,
    grid: &FilmGrid,
    id: String,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    state.update(cx, |state, _| {
        state.film_preview = grid.films.iter().find(|film| film.id == id).cloned()
    });
    let _ = shell.update(cx, |shell, cx| {
        shell.navigate(Route::Film { id }, window, cx)
    });
}

fn fixture_benchmark() -> Vec<FilmDetails> {
    let fixture = crate::fixtures::films();
    (0..200)
        .map(|index| {
            let mut film = fixture[index % fixture.len()].clone();
            film.id = format!("fixture:{index}");
            film
        })
        .collect()
}
