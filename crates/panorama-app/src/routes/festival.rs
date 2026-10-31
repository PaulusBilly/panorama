//! A festival or award's films, one edition at a time, from MUBI: its cover and white logo over
//! a row of years, then the film grid. MUBI's films carry no addon id, so opening one searches
//! the addons for it by title and year, and falls back to Search with the title typed in.
use crate::{
    alert_row::AlertRow,
    app::AppShell,
    app_state::AppState,
    args::Args,
    button::{Button, Icon},
    film_card,
    film_grid::{FilmGrid, GridHost, reveal},
    home_layout::{Grid, ViewSettings},
    image_cache::ImageCache,
    router::Route,
    theme::{
        Typography, caps, focus_ring,
        scrim::{Edge, scrim},
        typography::TITLE,
    },
    zoom::CardArt,
};
use futures::StreamExt;
use gpui::{
    Context, Entity, FocusHandle, ObjectFit, ScrollHandle, StyledImage, Subscription, Task,
    WeakEntity, Window, div, img, point, prelude::*, px, relative,
};
use panorama_core::{
    addons::{FilmDetails, ResourceEvent},
    credits::AwardResult,
    mubi::{Festival as Event, FestivalFilm},
};
use std::{collections::HashMap, sync::Arc};

/// The festival's white logo is drawn inside this box, as large as MUBI's.
const LOGO: (f32, f32) = (360.0, 140.0);
const GRID_GAP: f32 = 56.0;
/// One year tab: 8 above, a 13px label line, 6 below and the 2px underline.
const YEAR_ROW: f32 = 34.0;

/// What to do with a lookup's answer, once a card waiting on it was clicked.
struct Then {
    play: bool,
    card: Option<CardArt>,
    window: gpui::AnyWindowHandle,
}

struct Lookup {
    index: usize,
    /// Open (`false`) or play (`true`) once found; `None` for a lookup started on hover.
    then: Option<Then>,
    _task: Task<()>,
}

pub struct Festival {
    shell: WeakEntity<AppShell>,
    state: Entity<AppState>,
    /// History entry.
    id: u64,
    slug: String,
    /// The edition shown; `None` until the event says which is newest.
    year: Option<i32>,
    event: Option<Event>,
    films: Vec<FestivalFilm>,
    next_page: Option<u32>,
    loading: bool,
    error: Option<&'static str>,
    /// Each film's result at this edition, by MUBI film id.
    results: HashMap<u64, (AwardResult, Option<String>)>,
    /// The addon lookup in flight: started on hover, so a click usually finds it done.
    lookup: Option<Lookup>,
    /// Cards already found in the addons.
    found: HashMap<usize, FilmDetails>,
    grid: FilmGrid,
    scroll: ScrollHandle,
    images: Entity<ImageCache>,
    back: FocusHandle,
    retry: FocusHandle,
    more: FocusHandle,
    years: Vec<FocusHandle>,
    request: Option<Task<()>>,
    _images: Subscription,
    _shell: Option<Subscription>,
}

impl Festival {
    pub fn new(
        shell: WeakEntity<AppShell>,
        state: Entity<AppState>,
        args: Args,
        id: u64,
        slug: String,
        year: Option<i32>,
        cx: &mut Context<Self>,
    ) -> Self {
        let images = state.read(cx).images.clone();
        let image_observer = cx.observe(&images, |_, _, cx| cx.notify());
        let shell_observer = shell
            .upgrade()
            .map(|shell| cx.observe(&shell, |_, _, cx| cx.notify()));
        let mut festival = Self {
            shell,
            state,
            id,
            slug,
            year,
            event: None,
            films: vec![],
            next_page: None,
            loading: true,
            error: None,
            results: HashMap::new(),
            lookup: None,
            found: HashMap::new(),
            grid: FilmGrid::new(args.hover_first_card, cx),
            scroll: ScrollHandle::new(),
            images,
            back: cx.focus_handle(),
            retry: cx.focus_handle(),
            more: cx.focus_handle(),
            years: vec![],
            request: None,
            _images: image_observer,
            _shell: shell_observer,
        };
        festival.load(cx);
        festival
    }

    /// The event, then the first page of the edition; the event alone when it has no editions.
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        let Some(mubi) = services.mubi.clone() else {
            self.loading = false;
            self.error = Some("Festivals need a connection to MUBI.");
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        let known = self.event.clone();
        let (slug, year) = (self.slug.clone(), self.year);
        let mut job = crate::services::Job(services.runtime.spawn(async move {
            let event = match known {
                Some(event) => event,
                None => mubi.festival(&slug).await?,
            };
            let Some(year) = year.or_else(|| event.years.first().copied()) else {
                return Ok((event, None, None));
            };
            let (page, results) = futures::join!(
                mubi.festival_films(event.event.id, year, 1),
                mubi.festival_results(event.event.id, year),
            );
            // Without results the cards show no notes, rather than the page failing.
            Ok((
                event,
                Some(year),
                Some((page?, results.unwrap_or_default())),
            ))
        }));
        self.request = Some(cx.spawn(async move |entity, cx| {
            let result = (&mut job.0)
                .await
                .unwrap_or(Err(panorama_core::credits::CreditsError::Network));
            let _ = entity.update(cx, |festival, cx| {
                festival.loading = false;
                match result {
                    Ok((event, year, page)) => {
                        festival
                            .years
                            .resize_with(event.years.len(), || cx.focus_handle());
                        festival.event = Some(event);
                        festival.year = year;
                        festival.retitle(cx);
                        festival.films.clear();
                        festival.next_page = None;
                        if let Some((page, results)) = page {
                            festival.results = results;
                            festival.append(page, cx);
                        }
                    }
                    Err(_) => festival.error = Some("Could not load this festival. Try again."),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        let (Some(page), Some(year), Some(event)) = (self.next_page, self.year, &self.event) else {
            return;
        };
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        let Some(mubi) = services.mubi.clone() else {
            return;
        };
        let event = event.event.id;
        self.loading = true;
        let mut job = crate::services::Job(
            services
                .runtime
                .spawn(async move { mubi.festival_films(event, year, page).await }),
        );
        self.request = Some(cx.spawn(async move |entity, cx| {
            let result = (&mut job.0)
                .await
                .unwrap_or(Err(panorama_core::credits::CreditsError::Network));
            let _ = entity.update(cx, |festival, cx| {
                festival.loading = false;
                match result {
                    Ok(page) => festival.append(page, cx),
                    Err(_) => festival.error = Some("Could not load more films. Try again."),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn append(&mut self, page: panorama_core::mubi::FestivalPage, cx: &mut Context<Self>) {
        self.next_page = page.next_page;
        self.films.extend(page.films.into_iter().map(|mut film| {
            film.result = film.id.and_then(|id| self.results.get(&id).cloned());
            film
        }));
        let films: Arc<[FilmDetails]> = self
            .films
            .iter()
            .enumerate()
            .map(|(index, film)| details(index, film))
            .collect();
        let notes: Arc<[Option<String>]> = self.films.iter().map(note).collect();
        self.grid.set_films_with_notes(films, notes, cx);
    }

    fn pick_year(&mut self, year: i32, cx: &mut Context<Self>) {
        if self.year == Some(year) && self.error.is_none() {
            return;
        }
        self.year = Some(year);
        self.lookup = None;
        self.found.clear();
        self.results.clear();
        self.films.clear();
        self.next_page = None;
        self.grid.set_films(Arc::from([]), cx);
        self.retitle(cx);
        self.load(cx);
    }

    /// Record the edition on the history entry, so Back returns to it.
    fn retitle(&self, cx: &mut Context<Self>) {
        let (id, slug, year) = (self.id, self.slug.clone(), self.year);
        let _ = self.shell.update(cx, |shell, _| {
            shell.retitle(id, Route::Festival { slug, year });
        });
    }

    /// Open or play the card's film once the addons have it; Search when none does. A click
    /// while its lookup runs only updates what happens when it lands.
    fn resolve(
        &mut self,
        index: usize,
        play: bool,
        card: Option<CardArt>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(found) = self.found.get(&index).cloned() {
            self.finish(index, Some(found), play, card, window, cx);
            return;
        }
        if !self
            .lookup
            .as_ref()
            .is_some_and(|lookup| lookup.index == index)
        {
            self.look_up(index, cx);
        }
        if let Some(lookup) = &mut self.lookup {
            lookup.then = Some(Then {
                play,
                card,
                window: window.window_handle(),
            });
        }
        cx.notify();
    }

    /// Start finding card `index`'s film in the addons, replacing any lookup in flight.
    fn look_up(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(film) = self.films.get(index).cloned() else {
            return;
        };
        let state = self.state.read(cx);
        let Some(services) = state.services.clone() else {
            return;
        };
        let installed = state.installed.clone();
        let task = cx.spawn(async move |entity, cx| {
            // The first addon to have it wins; the rest are not waited for.
            let (job, receiver) = services.search_each(installed.clone(), film.title.clone());
            let mut found = first_match(&film, job, receiver).await;
            if found.is_none() {
                // Some catalogs leave out the year; the full search fills it in before matching.
                let (job, receiver) = services.search(installed, film.title.clone());
                found = first_match(&film, job, receiver).await;
            }
            let then = entity.update(cx, |festival, cx| {
                // A miss isn't kept: the next click asks again, with whatever addons are installed then.
                if let Some(found) = &found {
                    festival.found.insert(index, found.clone());
                }
                cx.notify();
                let lookup = festival.lookup.take_if(|lookup| lookup.index == index)?;
                lookup.then
            });
            if let Ok(Some(then)) = then {
                let _ = cx.update_window(then.window, |_, window, cx| {
                    let _ = entity.update(cx, |festival, cx| {
                        festival.finish(index, found, then.play, then.card, window, cx)
                    });
                });
            }
        });
        self.lookup = Some(Lookup {
            index,
            then: None,
            _task: task,
        });
    }

    /// The pointer rests on a card: look its film up now, so opening it is immediate.
    fn prefetch(&mut self, index: usize, cx: &mut Context<Self>) {
        let busy = self
            .lookup
            .as_ref()
            .is_some_and(|lookup| lookup.index == index || lookup.then.is_some());
        if !busy && !self.found.contains_key(&index) {
            self.look_up(index, cx);
        }
    }

    fn finish(
        &mut self,
        index: usize,
        found: Option<FilmDetails>,
        play: bool,
        card: Option<CardArt>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The user moved on while the addons answered; leave them where they are.
        let current = self
            .shell
            .upgrade()
            .is_some_and(|shell| shell.read(cx).is_current(self.id));
        if !current {
            return;
        }
        match found {
            Some(details) if play => {
                super::home::play_film(&self.state, &self.shell, details, None, window, cx)
            }
            Some(mut details) => match card {
                Some(card) => {
                    // The zoom lands on the page's backdrop, so the page keeps the card's still.
                    details.background = card.film.background.clone();
                    details.poster = card.film.poster.clone();
                    super::home::zoom_film(&self.state, &self.shell, details, card, window, cx)
                }
                None => super::home::open_film(&self.state, &self.shell, details, window, cx),
            },
            None => {
                let Some(query) = self.films.get(index).map(|film| film.title.clone()) else {
                    return;
                };
                let _ = self.shell.update(cx, |shell, cx| {
                    shell.navigate(Route::Search { query }, window, cx)
                });
            }
        }
        cx.notify();
    }

    fn opening(&self) -> bool {
        self.lookup
            .as_ref()
            .is_some_and(|lookup| lookup.then.is_some())
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
            keyboard,
            reduced,
        }
    }

    /// The cover fills a band this tall; the logo, title and years sit at its foot.
    fn hero_height(view: ViewSettings) -> f32 {
        (view.height * 0.62).clamp(420.0, 680.0)
    }

    fn grid_top(view: ViewSettings) -> f32 {
        Self::hero_height(view) + GRID_GAP
    }
}

/// An addon result that is the same film: the title folded equal and the year within one.
/// Anything less goes to Search, where the user picks, rather than opening a namesake.
/// The first search result that is `film`, reading `receiver` until one is or it ends.
async fn first_match(
    film: &FestivalFilm,
    _job: crate::services::Job<()>,
    mut receiver: futures::channel::mpsc::UnboundedReceiver<ResourceEvent<Vec<FilmDetails>>>,
) -> Option<FilmDetails> {
    while let Some(event) = receiver.next().await {
        match event {
            ResourceEvent::CacheHit(results) | ResourceEvent::Fresh(results) => {
                if let Some(found) = same_film(film, results) {
                    return Some(found);
                }
            }
            ResourceEvent::Failed(_) => return None,
        }
    }
    None
}

fn same_film(film: &FestivalFilm, results: Vec<FilmDetails>) -> Option<FilmDetails> {
    let title = fold(&film.title);
    let want = film.year?;
    results.into_iter().find(|result| {
        let named = fold(&result.name) == title
            || result
                .original_name
                .as_deref()
                .is_some_and(|name| fold(name) == title);
        let year = result.release_info.as_deref().and_then(release_year);
        named && year.is_some_and(|have| (want - have).abs() <= 1)
    })
}

fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The year a release line starts with: "2022", "2022-2023" or "2022-10-07".
pub(crate) fn release_year(info: &str) -> Option<i32> {
    info.trim().get(..4)?.parse().ok()
}

/// A MUBI film as a card. The id is a placeholder: opening a card resolves the real one.
fn details(index: usize, film: &FestivalFilm) -> FilmDetails {
    FilmDetails {
        id: format!("mubi-festival:{index}"),
        name: film.title.clone(),
        original_name: None,
        release_info: film.year.map(|year| year.to_string()),
        released: None,
        runtime: film.minutes.map(|minutes| format!("{minutes} min")),
        genres: film.genres.clone(),
        description: film.synopsis.clone(),
        director: film.directors.clone(),
        origin_country: None,
        cast: vec![],
        people: vec![],
        poster: None,
        background: film.still.clone(),
        logo: None,
        imdb_rating: None,
        links: vec![],
    }
}

/// The prize won ("Grand Prix") over a card's title, or "Runner-up · Grand Prix" for lesser
/// results; nothing for a film that was only nominated, since a category alone reads as a win.
fn note(film: &FestivalFilm) -> Option<String> {
    let (result, category) = film
        .result
        .as_ref()
        .filter(|(result, _)| *result != AwardResult::Nominated)?;
    let label = crate::award_marks::result_label(*result);
    Some(match (result, category) {
        (AwardResult::Won, Some(category)) => category.clone(),
        (_, Some(category)) => format!("{label} · {category}"),
        (AwardResult::Selected, None) => "Official Selection".into(),
        (_, None) => label.into(),
    })
}

impl GridHost for Festival {
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
            let year = self
                .event
                .as_ref()
                .and_then(|event| event.years.iter().position(|year| Some(*year) == self.year));
            match year.and_then(|year| self.years.get(year)) {
                Some(handle) => handle.focus(window, cx),
                None => self.back.focus(window, cx),
            }
        } else if forward && index >= last {
            if self.next_page.is_some() {
                self.more.focus(window, cx);
            } else {
                self.scroll.set_offset(point(px(0.0), px(0.0)));
                let _ = self
                    .shell
                    .update(cx, |shell, cx| shell.focus_sidebar(window, cx));
            }
        } else {
            let next = if forward { index + 1 } else { index - 1 };
            let y = Self::grid_top(view) + FilmGrid::row_top(next, col);
            reveal(&self.scroll, y, Grid::fit(col.width).height(), view.height);
            if let Some(card) = self.grid.focus(next) {
                card.focus(window, cx);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn hover_card(&mut self, index: usize, cx: &mut Context<Self>) {
        self.prefetch(index, cx);
    }
    fn open_film(&mut self, film: FilmDetails, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = card_index(&film) {
            self.resolve(index, false, None, window, cx);
        }
    }
    fn zoom_film(
        &mut self,
        film: FilmDetails,
        card: CardArt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = card_index(&film) {
            self.resolve(index, false, Some(card), window, cx);
        }
    }
    fn play_film(&mut self, film: FilmDetails, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = card_index(&film) {
            self.resolve(index, true, None, window, cx);
        }
    }
}

fn card_index(film: &FilmDetails) -> Option<usize> {
    film.id.strip_prefix("mubi-festival:")?.parse().ok()
}

impl Render for Festival {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.settings(window, cx);
        let theme = view.theme;
        let col = view.column();
        if view.active {
            self.images.update(cx, |images, _| images.begin_frame());
        }
        let services = self.state.read(cx).services.clone();
        let hero_height = Self::hero_height(view);
        let image = |url: Option<&url::Url>,
                     bounds: (f32, f32),
                     this: &mut Self,
                     cx: &mut Context<Self>| {
            let (url, services) = (url?, services.as_ref()?);
            if !view.active {
                return None;
            }
            this.images.update(cx, |images, cx| {
                images.request(url.as_str(), bounds, services, window, cx)
            })
        };
        let event = self.event.clone();
        let cover = image(
            event.as_ref().and_then(|event| event.event.cover.as_ref()),
            (view.width, hero_height),
            self,
            cx,
        );
        let logo = image(
            event.as_ref().and_then(|event| event.event.logo.as_ref()),
            LOGO,
            self,
            cx,
        );

        let mut hero = div()
            .relative()
            .w_full()
            .h(px(hero_height))
            .overflow_hidden()
            .bg(theme.artwork_empty);
        if let Some(cover) = cover {
            let fade = film_card::fade_in(&cover, view.reduced);
            hero = hero.child(
                crate::hero::cover(cover.image, (view.width, hero_height), false, 1.0)
                    .opacity(fade),
            );
        }
        hero = hero
            .child(scrim(theme, Edge::Top, relative(0.3), 0.6))
            .child(scrim(theme, Edge::Left, relative(0.6), 0.85))
            .child(scrim(theme, Edge::Bottom, relative(0.55), 1.0));

        let title = event.as_ref().map(|event| match self.year {
            Some(year) => format!("{} {year}", event.name),
            None => event.name.clone(),
        });
        let years = event
            .as_ref()
            .map(|event| event.years.clone())
            .unwrap_or_default();
        let tabs = years.iter().enumerate().map(|(index, &year)| {
            let selected = self.year == Some(year);
            let handle = self
                .years
                .get(index)
                .cloned()
                .unwrap_or_else(|| self.retry.clone());
            focus_ring(
                div()
                    .id(("festival-year", index))
                    .role(gpui::Role::Tab)
                    .aria_label(format!("{year}"))
                    .cursor_pointer()
                    .flex_none()
                    .px(px(10.0))
                    .pt(px(8.0))
                    .pb(px(6.0))
                    .border_b_2()
                    .border_color(if selected {
                        theme.ink
                    } else {
                        gpui::transparent_black().into()
                    })
                    .label()
                    .text_color(if selected { theme.ink } else { theme.ink_muted })
                    .hover(|style| style.text_color(theme.ink))
                    .child(year.to_string())
                    .on_click(cx.listener(move |festival, _, _, cx| festival.pick_year(year, cx))),
                &handle,
                theme,
                view.active,
                view.keyboard,
                0.0,
                window,
            )
        });
        let masthead = div()
            .absolute()
            .left(px(col.left))
            .bottom(px(0.0))
            .w(px(col.width))
            .flex()
            .flex_col()
            .items_start()
            .when_some(logo, |column, logo| {
                let fade = film_card::fade_in(&logo, view.reduced);
                // Sized to the logo's own shape, so it sits flush with the title's left edge.
                let size = logo.image.size(0);
                let aspect = size.width.0 as f32 / size.height.0.max(1) as f32;
                let width = LOGO.0.min(LOGO.1 * aspect);
                column.child(
                    img(logo.image)
                        .w(px(width))
                        .h(px(width / aspect))
                        .object_fit(ObjectFit::Contain)
                        .opacity(fade),
                )
            })
            .when_some(title, |column, title| {
                column.child(
                    TITLE
                        .apply(div())
                        .mt(px(20.0))
                        .text_color(theme.ink)
                        .child(caps(&title)),
                )
            })
            .when(!years.is_empty(), |column| {
                column.child(
                    div()
                        .id("festival-years")
                        .role(gpui::Role::TabList)
                        .aria_label("Editions")
                        .mt(px(20.0))
                        .ml(px(-10.0))
                        .max_w(px(col.width + 10.0))
                        // A scroll container takes no height from its row; this is one tab.
                        .h(px(YEAR_ROW))
                        .overflow_x_scroll()
                        .flex()
                        .children(tabs),
                )
            });
        hero = hero.child(masthead).child(
            div()
                .absolute()
                .top(px(
                    crate::sidebar::LOGO_CENTER - crate::button::Size::Regular.height() * 0.5
                ))
                .left(px(col.left))
                .child(
                    Button::round("festival-back", Icon::ArrowLeft, "Back")
                        .focus(&self.back, view.keyboard)
                        .tab_stop(view.active)
                        .on_click(cx.listener(|festival, _, window, cx| {
                            let entry = festival.id;
                            let _ = festival.shell.update(cx, |shell, cx| {
                                shell.back(window, cx);
                                if shell.is_current(entry) {
                                    shell.go(crate::router::Destination::Home, window, cx);
                                }
                            });
                        })),
                ),
        );

        let mut section = div()
            .ml(px(col.left))
            .w(px(col.width))
            .pt(px(GRID_GAP))
            .pb(px(112.0));
        if let Some(message) = self.error {
            section = section.child(
                AlertRow {
                    message: message.into(),
                    label: "Try Again",
                    theme,
                    width: view.width,
                    focus: &self.retry,
                    active: view.active,
                    keyboard: view.keyboard,
                }
                .render(cx.listener(|festival, _, _, cx| {
                    if festival.films.is_empty() {
                        festival.load(cx);
                    } else {
                        festival.error = None;
                        festival.load_more(cx);
                    }
                })),
            );
        }
        if event.is_some() && self.films.is_empty() && !self.loading && self.error.is_none() {
            section = section.child(
                div()
                    .label()
                    .text_color(theme.ink_muted)
                    .child("MUBI lists no films for this edition."),
            );
        } else if self.error.is_none() || !self.films.is_empty() {
            let top = -f32::from(self.scroll.offset().y);
            let offset = Self::grid_top(view) - top;
            let images = self.images.clone();
            self.grid.quiet = self.state.read(cx).quiet.clone();
            section = section.child(self.grid.render_rows(
                offset,
                self.loading && self.films.is_empty(),
                view,
                &images,
                services.as_ref(),
                window,
                cx,
            ));
        }
        if self.next_page.is_some() {
            section = section.child(
                div().mt(px(40.0)).flex().child(
                    Button::glass("festival-more", "Load More")
                        .compact()
                        .busy(self.loading)
                        .focus(&self.more, view.keyboard)
                        .tab_stop(view.active)
                        .on_click(cx.listener(|festival, _, _, cx| festival.load_more(cx))),
                ),
            );
        }
        if view.active {
            self.images.update(cx, |images, _| images.finish_frame());
        }
        if self.loading
            || self.opening()
            || self.grid.moving(view.reduced)
            || (!view.reduced && view.active && self.images.read(cx).moving())
        {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        let status = match (self.opening(), self.loading) {
            (true, _) => "Finding this film in your addons".to_owned(),
            (false, true) => "Loading films".to_owned(),
            _ => String::new(),
        };
        div()
            .id(("festival-scroll", self.id))
            .size_full()
            .overflow_y_scroll()
            // Sideways swipes belong to the carousels; without this the page turns them
            // into vertical scrolling.
            .restrict_scroll_to_axis()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
            .bg(theme.canvas)
            .text_color(theme.ink)
            .child(hero)
            .child(section)
            .child(crate::home_layout::footer())
            .child(crate::a11y::status("festival-status", status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film(title: &str, year: Option<i32>) -> FestivalFilm {
        FestivalFilm {
            id: None,
            title: title.into(),
            year,
            directors: vec![],
            still: None,
            synopsis: None,
            minutes: None,
            genres: vec![],
            result: None,
        }
    }

    fn result(id: &str, name: &str, release: &str) -> FilmDetails {
        let mut film = details(0, &film(name, None));
        film.id = id.into();
        film.release_info = Some(release.into());
        film
    }

    #[test]
    fn a_festival_film_finds_its_addon_match() {
        let results = || {
            vec![
                result("tt1", "Aftersun", "2006"),
                result("tt2", "Aftersun: Revisited", "2022"),
                result("tt3", "Aftersun", "2022"),
            ]
        };
        let found = same_film(&film("Aftersun", Some(2022)), results());
        assert_eq!(found.map(|film| film.id).as_deref(), Some("tt3"));
        assert!(same_film(&film("Aftersun", Some(2015)), results()).is_none());
        assert!(same_film(&film("Aftersun", None), results()).is_none());
        // A same-year result under another title is not opened.
        let revisited = vec![result("tt2", "Aftersun: Revisited", "2022")];
        assert!(same_film(&film("Aftersun", Some(2022)), revisited).is_none());
    }

    #[test]
    fn notes_and_card_ids() {
        let mut winner = film("Close", Some(2022));
        winner.result = Some((AwardResult::Won, Some("Grand Prix".into())));
        assert_eq!(note(&winner).as_deref(), Some("Grand Prix"));
        winner.result = Some((AwardResult::RunnerUp, Some("Golden Lion".into())));
        assert_eq!(note(&winner).as_deref(), Some("Runner-up · Golden Lion"));
        winner.result = Some((AwardResult::Selected, None));
        assert_eq!(note(&winner).as_deref(), Some("Official Selection"));
        winner.result = Some((AwardResult::Nominated, Some("Best Film".into())));
        assert_eq!(note(&winner), None);
        assert_eq!(note(&film("Close", None)), None);
        assert_eq!(card_index(&details(7, &winner)), Some(7));
        assert_eq!(release_year("2022-10-07"), Some(2022));
    }
}
