use crate::{
    app::AppShell,
    app_state::AppState,
    args::Args,
    home_layout::ViewSettings,
    image_cache::{CachedImage, ImageCache, render_image},
    router::Route,
    services::Job,
    theme::motion::*,
    transition::Tween,
};
use futures::{StreamExt, channel::mpsc};
use gpui::{
    Context, Entity, FocusHandle, RenderImage, ScrollHandle, Subscription, Task, WeakEntity,
    Window, point, prelude::*, px,
};
use panorama_core::addons::FilmDetails;
use std::{collections::HashMap, sync::Arc, time::Instant};

gpui::actions!(panorama_card, [NextCard, PreviousCard]);

#[path = "home_alert.rs"]
mod alert;
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
    pub(crate) films: Arc<[FilmDetails]>,
    pub(crate) cards: Vec<FocusHandle>,
    pub(crate) search: Option<super::search::SearchData>,
    watch: FocusHandle,
    retry: FocusHandle,
    reload: FocusHandle,
    more: FocusHandle,
    images: Entity<ImageCache>,
    hovered: HashMap<usize, Tween>,
    overlay: Option<Arc<RenderImage>>,
    overlay_size: (u32, u32),
    overlay_task: Option<Task<()>>,
    surface: Tween,
    ready: bool,
    watch_hover: Tween,
    last_top: f32,
    launched: Instant,
    debug_step: usize,
    debug_focus: bool,
    benchmark: Option<crate::debug::performance::Samples>,
    _watch_focus: Option<Subscription>,
    _images: Subscription,
    _catalog: Subscription,
    _shell: Subscription,
    _release: Subscription,
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
            if home.search.as_ref().is_some_and(|search| {
                search.installed != state.read(cx).installed
                    || search.account != state.read(cx).account
            }) {
                home.load_search(cx);
            }
            if !home.args.bench_scroll && home.search.is_none() {
                home.films = state
                    .read(cx)
                    .catalog
                    .as_ref()
                    .map(|p| p.films.clone())
                    .unwrap_or_default()
                    .into();
            }
            home.cards.truncate(home.films.len());
            while home.cards.len() < home.films.len() {
                home.cards.push(cx.focus_handle());
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
        let release = cx.on_release(|home, cx| {
            if let Some(image) = home.overlay.take() {
                cx.drop_image(image, None);
            }
        });
        let watch = cx.focus_handle();
        Self {
            shell,
            state,
            args,
            id,
            scroll: ScrollHandle::new(),
            cards: (0..films.len()).map(|_| cx.focus_handle()).collect(),
            films,
            search: None,
            watch,
            retry: cx.focus_handle(),
            reload: cx.focus_handle(),
            more: cx.focus_handle(),
            images,
            hovered: HashMap::new(),
            overlay: None,
            overlay_size: (0, 0),
            overlay_task: None,
            surface: Tween::fixed(1.0),
            ready: false,
            watch_hover: Tween::fixed(0.0),
            last_top: 0.0,
            launched: Instant::now(),
            debug_step: 0,
            debug_focus: false,
            benchmark: None,
            _watch_focus: None,
            _images: image_observer,
            _catalog: observer,
            _shell: shell_observer,
            _release: release,
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
            height: f32::from(window.viewport_size().height)
                - if window.is_fullscreen() {
                    0.0
                } else if cfg!(target_os = "macos") {
                    38.0
                } else {
                    32.0
                },
            theme,
            active,
            keyboard: keyboard || self.args.focus_first_card,
            reduced,
        }
    }
    fn ensure_overlay(&mut self, view: ViewSettings, window: &Window, cx: &mut Context<Self>) {
        let scale = window.scale_factor();
        let size = (
            (view.width * scale).ceil().min(4096.0) as u32,
            (view.height * scale).ceil().min(4096.0) as u32,
        );
        if size == self.overlay_size {
            return;
        }
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        self.overlay_size = size;
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                render_image(crate::hero::overlay(size.0, size.1, view.theme))
            })
            .await;
            let _ = sender.unbounded_send(result.ok().and_then(Result::ok));
        }));
        self.overlay_task = Some(cx.spawn_in(window, async move |entity, cx| {
            let _job = job;
            if let Some(image) = receiver.next().await {
                let _ = cx.update(|window, cx| {
                    entity.update(cx, |home, cx| {
                        if let Some(old) = home.overlay.take() {
                            cx.drop_image(old, Some(window));
                        }
                        home.overlay = image;
                        cx.notify();
                    })
                });
            }
        }));
    }
    fn artwork(
        &mut self,
        film: &FilmDetails,
        bounds: (f32, f32),
        visible: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Option<CachedImage>, bool, bool) {
        let Some(services) = self.state.read(cx).services.clone() else {
            return (None, false, true);
        };
        for (url, poster) in [(&film.background, false), (&film.poster, true)] {
            if let Some(url) = url {
                if !visible {
                    return (
                        self.images.update(cx, |images, _| {
                            images.cached(url.as_str(), bounds, window.scale_factor())
                        }),
                        poster,
                        false,
                    );
                }

                if self.images.update(cx, |images, _| {
                    images.failed(url.as_str(), bounds, window.scale_factor())
                }) {
                    continue;
                }
                return (
                    self.images.update(cx, |images, cx| {
                        images.request(url.as_str(), bounds, &services, window, cx)
                    }),
                    poster,
                    false,
                );
            }
        }
        (None, false, true)
    }
    /// Retarget a card's image expansion without changing tile layout.
    pub(crate) fn hover(&mut self, index: usize, over: bool, cx: &mut Context<Self>) {
        self.hovered
            .entry(index)
            .or_insert_with(|| Tween::fixed(0.0))
            .retarget(
                if over { 1.0 } else { 0.0 },
                DURATION_STANDARD,
                Some(EASE_EDITORIAL),
                Instant::now(),
            );
        cx.notify();
    }
    pub(crate) fn hover_watch(&mut self, over: bool, cx: &mut Context<Self>) {
        self.watch_hover.retarget(
            if over { 1.0 } else { 0.0 },
            DURATION_FAST,
            Some(EASE_DEFAULT),
            Instant::now(),
        );
        cx.notify();
    }
    /// Open the card's opaque film identifier in the existing router.
    pub(crate) fn open_film(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, _| {
            state.film_preview = self.films.iter().find(|film| film.id == id).cloned()
        });
        let _ = self.shell.update(cx, |shell, cx| {
            shell.navigate(Route::Film { id }, window, cx)
        });
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
            && !self.cards.is_empty()
            && self.debug_step == self.args.scroll.len()
        {
            self.cards[0].focus(window, cx);
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
            let height = if self.search.is_some() {
                0.0
            } else {
                self.content_start(view, cx)
            };
            let _ = self.shell.update(cx, |shell, cx| {
                shell.menu.dismiss();
                shell.update_home_header(self.id, top, height, self.ready, cx);
            });
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
