use crate::{
    app::AppShell,
    app_state::{Account, AppState},
    args::Args,
    home_layout::ViewSettings,
    image_cache::{CachedImage, ImageCache},
    services::Job,
};
use gpui::{
    Context, Entity, FocusHandle, ScrollHandle, Subscription, Task, WeakEntity, Window,
};
use panorama_core::addons::{FilmDetails, StreamGroup};
use std::{sync::Arc, time::Instant};

#[path = "film_actions.rs"]
mod actions;
#[path = "film_data.rs"]
mod data;
#[path = "film_images.rs"]
mod images;
#[path = "film_view.rs"]
mod view;

/// Retained Film metadata and independent source, library and renderer lifetimes.
pub struct Film {
    shell: WeakEntity<AppShell>,
    state: Entity<AppState>,
    args: Args,
    entry: u64,
    id: String,
    scroll: ScrollHandle,
    metadata: Option<FilmDetails>,
    loading: bool,
    details_error: bool,
    groups: Vec<StreamGroup>,
    selected: Option<crate::playback::selection::SourceRef>,
    resume: Option<panorama_core::stremio::ResumePoint>,
    meta_base: Option<url::Url>,
    sources_loading: bool,
    sources_error: bool,
    saved: bool,
    saving: bool,
    watchlist_error: bool,
    account: Account,
    installed: Vec<panorama_core::stremio::Descriptor>,
    back: FocusHandle,
    primary: FocusHandle,
    restart: FocusHandle,
    watchlist: FocusHandle,
    retry_details: FocusHandle,
    retry_sources: FocusHandle,
    /// Opened from Home's play button: play once a source is ready.
    autoplay: bool,
    launched: Instant,
    debug_step: usize,
    images: Entity<ImageCache>,
    details_task: Option<Task<()>>,
    sources_task: Option<Task<()>>,
    library_task: Option<Task<()>>,
    save_task: Option<Task<()>>,
    _state: Subscription,
    _shell: Subscription,
    _images: Subscription,
}
impl Film {
    /// Start details and sources together, with the clicked card as a placeholder.
    pub fn new(
        id: String,
        entry: u64,
        shell: WeakEntity<AppShell>,
        state: Entity<AppState>,
        args: Args,
        cx: &mut Context<Self>,
    ) -> Self {
        let intent = state.update(cx, |state, _| {
            state.play_intent.take_if(|intent| intent.id == id)
        });
        let current = state.read(cx);
        let metadata = current
            .film_preview
            .as_ref()
            .filter(|film| film.id == id)
            .cloned()
            .or_else(|| {
                current
                    .catalog
                    .as_ref()?
                    .films
                    .iter()
                    .find(|film| film.id == id)
                    .cloned()
            });
        let images = current.images.clone();
        let account = current.account.clone();
        let installed = current.installed.clone();
        let observer = cx.observe(&state, |film, state, cx| {
            let state = state.read(cx);
            let changed = film.account != state.account || film.installed != state.installed;
            film.account = state.account.clone();
            film.installed = state.installed.clone();
            if changed {
                film.load_sources(cx);
                film.subscribe_library(cx);
            }
            cx.notify();
        });
        let shell_observer = shell
            .upgrade()
            .map(|shell| cx.observe(&shell, |_, _, cx| cx.notify()))
            .unwrap_or_else(|| cx.on_release(|_, _| {}));
        let image_observer = cx.observe(&images, |_, _, cx| cx.notify());
        let mut film = Self {
            shell,
            state,
            args,
            entry,
            id,
            scroll: ScrollHandle::new(),
            metadata,
            loading: true,
            details_error: false,
            groups: vec![],
            selected: None,
            resume: intent.as_ref().and_then(|intent| intent.resume),
            meta_base: None,
            sources_loading: true,
            sources_error: false,
            saved: false,
            saving: false,
            watchlist_error: false,
            account,
            installed,
            back: cx.focus_handle(),
            primary: cx.focus_handle(),
            restart: cx.focus_handle(),
            watchlist: cx.focus_handle(),
            retry_details: cx.focus_handle(),
            retry_sources: cx.focus_handle(),
            autoplay: intent.is_some(),
            launched: Instant::now(),
            debug_step: 0,
            images,
            details_task: None,
            sources_task: None,
            library_task: None,
            save_task: None,
            _state: observer,
            _shell: shell_observer,
            _images: image_observer,
        };
        film.load_details(cx);
        film.load_sources(cx);
        film.subscribe_library(cx);
        film
    }
    fn settings(&self, window: &Window, cx: &Context<Self>) -> ViewSettings {
        let (theme, active, keyboard, reduced) = self
            .shell
            .upgrade()
            .map(|shell| {
                let shell = shell.read(cx);
                (
                    shell.theme,
                    shell.is_current(self.entry),
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
            keyboard,
            reduced,
        }
    }
}
