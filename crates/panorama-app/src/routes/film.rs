use crate::{
    app::AppShell,
    app_state::{Account, AppState},
    args::Args,
    home_layout::ViewSettings,
    image_cache::{CachedImage, ImageCache},
    services::Job,
};
use gpui::{Context, Entity, FocusHandle, ScrollHandle, Subscription, Task, WeakEntity, Window};
use panorama_core::addons::{FilmDetails, StreamGroup};
use std::{sync::Arc, time::Instant};

#[path = "film_actions.rs"]
mod actions;
#[path = "film_credits.rs"]
mod credits;
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
    /// The addons' subtitle files; `None` while they are looked up. Play waits for them so
    /// they are in the player before the first frame.
    subtitles: Option<Vec<panorama_core::addons::SubtitleSource>>,
    subtitle_lookup: Option<crate::playback::selection::SubtitleLookup>,
    saved: bool,
    saving: bool,
    watchlist_error: bool,
    /// Awards below the hero; empty until they arrive or when neither MUBI nor Wikidata has any.
    awards: Vec<panorama_core::credits::Award>,
    /// The Awards & Festivals carousel.
    awards_rail: crate::carousel::Carousel,
    /// One per tile shown, for the tiles that open a festival page.
    award_tiles: Vec<FocusHandle>,
    /// One per tile shown, for its "+N More".
    award_more: Vec<FocusHandle>,
    /// Tiles opened past their first lines, by index.
    awards_expanded: std::collections::HashSet<usize>,
    /// The Cast & Crew carousel.
    people_rail: crate::carousel::Carousel,
    account: Account,
    installed: Vec<panorama_core::stremio::Descriptor>,
    back: FocusHandle,
    primary: FocusHandle,
    watchlist: FocusHandle,
    retry_details: FocusHandle,
    launched: Instant,
    /// When everything was in, or the wait ran out; the page holds on its banner until then.
    revealed: Option<Instant>,
    /// The foreign title lookup is still out.
    original_pending: bool,
    /// The title in the film's own language, when its metadata has none.
    original_title: Option<String>,
    /// The card this page was opened from.
    card: Option<FilmDetails>,
    /// The color of the card this page was opened from, if its artwork is still cached.
    tint: Option<gpui::Hsla>,
    debug_step: usize,
    images: Entity<ImageCache>,
    details_task: Option<Task<()>>,
    sources_seen: Option<Arc<Vec<panorama_core::addons::StreamGroup>>>,
    subtitles_task: Option<Task<()>>,
    library_task: Option<Task<()>>,
    save_task: Option<Task<()>>,
    credits_task: Option<Task<()>>,
    original_task: Option<Task<()>>,
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
        let card = metadata.clone();
        let images = current.images.clone();
        let tint = metadata.as_ref().and_then(|film| {
            [&film.background, &film.poster]
                .into_iter()
                .flatten()
                .find_map(|url| images.read(cx).tint(url.as_str()))
        });
        let awards_expanded = if args.awards_expand_first {
            std::collections::HashSet::from([0])
        } else {
            Default::default()
        };
        let account = current.account.clone();
        let installed = current.installed.clone();
        let observer = cx.observe(&state, |film, state, cx| {
            let state = state.read(cx);
            let changed = film.account != state.account || film.installed != state.installed;
            film.account = state.account.clone();
            film.installed = state.installed.clone();
            film.sync_sources(state);
            if changed {
                film.load_sources(true, cx);
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
            resume: None,
            meta_base: None,
            sources_loading: true,
            subtitles: None,
            subtitle_lookup: None,
            sources_error: false,
            saved: false,
            saving: false,
            watchlist_error: false,
            awards: vec![],
            awards_rail: Default::default(),
            award_tiles: vec![],
            award_more: vec![],
            awards_expanded,
            people_rail: Default::default(),
            account,
            installed,
            back: cx.focus_handle(),
            primary: cx.focus_handle(),
            watchlist: cx.focus_handle(),
            retry_details: cx.focus_handle(),
            launched: Instant::now(),
            revealed: None,
            original_pending: false,
            original_title: None,
            card,
            tint,
            debug_step: 0,
            images,
            details_task: None,
            sources_seen: None,
            subtitles_task: None,
            library_task: None,
            save_task: None,
            credits_task: None,
            original_task: None,
            _state: observer,
            _shell: shell_observer,
            _images: image_observer,
        };
        film.load_details(cx);
        film.load_sources(false, cx);
        film.load_credits(cx);
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
