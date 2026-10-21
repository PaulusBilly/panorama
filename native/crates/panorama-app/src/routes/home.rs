pub(crate) use crate::film_grid::{NextCard, PreviousCard};
use crate::{
    app::AppShell,
    app_state::{Account, AppState},
    args::Args,
    continue_row::ContinueRow,
    film_grid::{FilmGrid, GridHost},
    hero::HeroFocus,
    home_layout::ViewSettings,
    image_cache::{CachedImage, ImageCache},
    router::Route,
    services::Job,
};
use futures::{StreamExt, channel::mpsc};
use gpui::{
    Context, Entity, FocusHandle, ScrollHandle, Subscription, Task, WeakEntity, Window, point,
    prelude::*, px,
};
use panorama_core::{addons::FilmDetails, stremio::FilmMeta};
use std::{sync::Arc, time::Instant};

#[path = "home_alert.rs"]
mod alert;
#[path = "home_catalog.rs"]
mod catalog;
#[path = "home_keyboard.rs"]
mod keyboard;
#[path = "home_lineup.rs"]
pub(crate) mod lineup;
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
    hero: HeroFocus,
    row: ContinueRow,
    fold: f32,
    flags: Option<(String, Task<()>)>,
    saved: bool,
    save_task: Option<Task<()>>,
    switcher: crate::catalog_switcher::CatalogSwitcher,
    retry: FocusHandle,
    reload: FocusHandle,
    more: FocusHandle,
    images: Entity<ImageCache>,
    last_top: f32,
    launched: Instant,
    debug_step: usize,
    debug_focus: bool,
    benchmark: Option<crate::debug::performance::Samples>,
    _watch_focus: Option<Subscription>,
    _images: Subscription,
    _continue: Subscription,
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
        let continue_observer = cx
            .observe(&state.read(cx).continue_watching.clone(), |_, _, cx| {
                cx.notify()
            });
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
            hero: HeroFocus::new(cx),
            row: ContinueRow::default(),
            fold: 0.0,
            flags: None,
            saved: false,
            save_task: None,
            switcher,
            retry: cx.focus_handle(),
            reload: cx.focus_handle(),
            more: cx.focus_handle(),
            images,
            last_top: 0.0,
            launched: Instant::now(),
            debug_step: 0,
            debug_focus: false,
            benchmark: None,
            _watch_focus: None,
            _images: image_observer,
            _continue: continue_observer,
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
    fn signed_in(&self, cx: &Context<Self>) -> bool {
        matches!(self.state.read(cx).account, Account::SignedIn(_))
    }
    /// Open the film and let it start playback, since only Film resolves a playable source.
    pub(crate) fn play(
        &mut self,
        film: FilmDetails,
        resume: Option<panorama_core::stremio::ResumePoint>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        play_film(&self.state, &self.shell, film, resume, window, cx);
    }
    fn track_saved(&mut self, id: Option<&str>, cx: &mut Context<Self>) {
        if self.flags.as_ref().map(|(tracked, _)| tracked.as_str()) == id {
            return;
        }
        self.saved = false;
        self.save_task = None;
        self.flags = None;
        let (Some(id), Some(services)) = (id, self.state.read(cx).services.clone()) else {
            return;
        };
        let (job, mut receiver) = services.library_flags(id.to_string());
        let task = cx.spawn(async move |home, cx| {
            let _job = job;
            while let Some((saved, _)) = receiver.next().await {
                if home
                    .update(cx, |home, cx| {
                        home.saved = saved;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        self.flags = Some((id.to_string(), task));
    }
    /// Add the hero film to the watchlist, or remove it. Fixtures have no session and succeed.
    pub(crate) fn toggle_saved(&mut self, film: &FilmDetails, cx: &mut Context<Self>) {
        if self.save_task.is_some() {
            return;
        }
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        let saved = !self.saved;
        let meta = FilmMeta {
            id: film.id.clone(),
            name: film.name.clone(),
            poster: film.poster.clone(),
        };
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.clone().spawn(async move {
            let result = match &services.session {
                Some(session) => session.lock().await.set_in_library(&meta, saved).is_ok(),
                None => true,
            };
            let _ = sender.unbounded_send(result);
        }));
        self.save_task = Some(cx.spawn(async move |home, cx| {
            let _job = job;
            let success = receiver.next().await.unwrap_or(false);
            let _ = home.update(cx, |home, cx| {
                home.save_task = None;
                if success {
                    home.saved = saved;
                }
                cx.notify();
            });
        }));
        cx.notify();
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
            && !self.grid.is_empty()
            && self.debug_step == self.args.scroll.len()
        {
            self.debug_focus = true;
            self.focus_first_card(window, cx);
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
    fn open_film(&mut self, film: FilmDetails, window: &mut Window, cx: &mut Context<Self>) {
        open_film(&self.state, &self.shell, film, window, cx);
    }
    fn play_film(&mut self, film: FilmDetails, window: &mut Window, cx: &mut Context<Self>) {
        self.play(film, None, window, cx);
    }
}

/// Open the film with a play intent; Film starts playback through its own source selection.
pub(crate) fn play_film<V: 'static>(
    state: &Entity<AppState>,
    shell: &WeakEntity<AppShell>,
    film: FilmDetails,
    resume: Option<panorama_core::stremio::ResumePoint>,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let id = film.id.clone();
    state.update(cx, |state, _| {
        state.play_intent = Some(crate::app_state::PlayIntent { id, resume })
    });
    open_film(state, shell, film, window, cx);
}

pub(crate) fn open_film<V: 'static>(
    state: &Entity<AppState>,
    shell: &WeakEntity<AppShell>,
    film: FilmDetails,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let id = film.id.clone();
    state.update(cx, |state, _| state.film_preview = Some(film));
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
