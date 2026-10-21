use crate::{
    film_card::{self, CardProps},
    home_layout::{GAP, Grid, ViewSettings},
    image_cache::{CachedImage, ImageCache},
    services::Services,
    theme::{Column, motion::*},
    transition::Tween,
};
use gpui::{
    App, Context, Div, Entity, FocusHandle, ScrollHandle, Window, div, point, prelude::*, px,
};
use panorama_core::addons::FilmDetails;
use std::{collections::HashMap, sync::Arc, time::Instant};

gpui::actions!(panorama_card, [NextCard, PreviousCard]);

const SKELETONS: usize = 9;

/// A view that owns a `FilmGrid`; card listeners call back into it.
pub trait GridHost: Sized + 'static {
    fn film_grid(&mut self) -> &mut FilmGrid;
    /// Move focus off card `index`, forward or backward, wrapping at the grid's ends.
    fn focus_card(
        &mut self,
        index: usize,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
    fn open_film(&mut self, film: FilmDetails, window: &mut Window, cx: &mut Context<Self>);
}

/// Virtualised film-card grid with focus handles and hover tweens. Owned per entity; no sharing.
pub struct FilmGrid {
    pub films: Arc<[FilmDetails]>,
    cards: Vec<FocusHandle>,
    spare: FocusHandle,
    hovered: HashMap<usize, Tween>,
    hover_first: bool,
    launched: Instant,
}

impl FilmGrid {
    /// An empty grid; `hover_first` pins the first card's hover look for captures.
    pub fn new(hover_first: bool, cx: &mut App) -> Self {
        Self {
            films: Arc::from([]),
            cards: Vec::new(),
            spare: cx.focus_handle(),
            hovered: HashMap::new(),
            hover_first,
            launched: Instant::now(),
        }
    }
    /// Replace the films, keeping one focus handle per card.
    pub fn set_films(&mut self, films: Arc<[FilmDetails]>, cx: &mut App) {
        self.cards.resize_with(films.len(), || cx.focus_handle());
        self.films = films;
    }
    pub fn focus(&self, index: usize) -> Option<&FocusHandle> {
        self.cards.get(index)
    }
    pub fn len(&self) -> usize {
        self.cards.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }
    /// Top of card `index`'s row, relative to the grid's top edge.
    pub fn row_top(index: usize, col: Column) -> f32 {
        let grid = Grid::fit(col.width);
        (index / grid.columns) as f32 * grid.stride()
    }
    /// Retarget a card's image expansion without changing tile layout.
    pub fn hover(&mut self, index: usize, over: bool) {
        self.hovered
            .entry(index)
            .or_insert_with(|| Tween::fixed(0.0))
            .retarget(
                if over { 1.0 } else { 0.0 },
                DURATION_STANDARD,
                Some(EASE_EDITORIAL),
                Instant::now(),
            );
    }
    pub fn moving(&self, reduced: bool) -> bool {
        let now = Instant::now();
        self.hovered.values().any(|t| t.moving(now, reduced))
    }
    /// Render the rows that intersect the viewport. `offset` is the grid's top edge relative to
    /// the viewport's; `skeletons` draws placeholder tiles while there are no films yet.
    #[allow(clippy::too_many_arguments)]
    pub fn render_rows<V: GridHost>(
        &mut self,
        offset: f32,
        skeletons: bool,
        view: ViewSettings,
        images: &Entity<ImageCache>,
        services: Option<&Arc<Services>>,
        window: &Window,
        cx: &mut Context<V>,
    ) -> Div {
        let grid = Grid::fit(view.column().width);
        let count = if skeletons && self.films.is_empty() {
            SKELETONS
        } else {
            self.films.len()
        };
        let rows = count.div_ceil(grid.columns);
        let height = (rows as f32 * grid.stride() - if rows > 0 { grid.row_gap() } else { 0.0 }).max(0.0);
        let start = (-offset / grid.stride()).floor().max(0.0) as usize;
        let start = start.saturating_sub(1).min(rows);
        let end = (((view.height - offset) / grid.stride()).ceil().max(0.0) as usize + 2).min(rows);
        let phase = if view.reduced {
            0.0
        } else {
            (Instant::now().duration_since(self.launched).as_secs_f32()
                / SHIMMER.duration.as_secs_f32())
                % 1.0
        };
        let films = self.films.clone();
        let mut result = div().relative().w(px(grid.total_width())).h(px(height));
        for row in start..end {
            let y = offset + row as f32 * grid.stride();
            let visible = view.active && y + grid.height() > 0.0 && y < view.height;
            let mut tiles = div()
                .absolute()
                .top(px(row as f32 * grid.stride()))
                .left_0()
                .w_full()
                .h(px(grid.height()))
                .flex()
                .gap(px(GAP));
            for index in row * grid.columns..((row + 1) * grid.columns).min(count) {
                let film = films.get(index);
                let (image, poster, unavailable) = film.map_or((None, false, false), |film| {
                    artwork(
                        images,
                        services,
                        film,
                        (grid.width, grid.height()),
                        visible,
                        window,
                        cx,
                    )
                });
                let hover = if self.hover_first && index == 0 {
                    1.0
                } else {
                    self.hovered
                        .get(&index)
                        .map_or(0.0, |t| t.value(Instant::now(), view.reduced))
                };
                tiles = tiles.child(film_card::render(
                    CardProps {
                        film,
                        image,
                        poster,
                        unavailable,
                        focus: self.cards.get(index).unwrap_or(&self.spare),
                        hover,
                        index,
                        shimmer: phase,
                    },
                    grid,
                    view,
                    window,
                    cx,
                ));
            }
            result = result.child(tiles);
        }
        self.hovered
            .retain(|index, _| *index >= start * grid.columns && *index < end * grid.columns);
        result
    }
}

/// Pick a film's backdrop, falling back to its poster, and request it when `visible`.
/// Returns the image, whether it is the poster, and whether no artwork exists.
pub fn artwork(
    images: &Entity<ImageCache>,
    services: Option<&Arc<Services>>,
    film: &FilmDetails,
    bounds: (f32, f32),
    visible: bool,
    window: &Window,
    cx: &mut App,
) -> (Option<CachedImage>, bool, bool) {
    let Some(services) = services else {
        return (None, false, true);
    };
    for (url, poster) in [(&film.background, false), (&film.poster, true)] {
        if let Some(url) = url {
            if !visible {
                return (
                    images.update(cx, |images, _| {
                        images.cached(url.as_str(), bounds, window.scale_factor())
                    }),
                    poster,
                    false,
                );
            }
            if images.update(cx, |images, _| {
                images.failed(url.as_str(), bounds, window.scale_factor())
            }) {
                continue;
            }
            return (
                images.update(cx, |images, cx| {
                    images.request(url.as_str(), bounds, services, window, cx)
                }),
                poster,
                false,
            );
        }
    }
    (None, false, true)
}

/// Scroll the least distance that shows the band `y..y + height` of the scroll content.
pub fn reveal(scroll: &ScrollHandle, y: f32, height: f32, viewport: f32) {
    let top = -f32::from(scroll.offset().y);
    if y < top {
        scroll.set_offset(point(px(0.0), px(-y)));
    } else if y + height > top + viewport {
        scroll.set_offset(point(px(0.0), px(-(y + height - viewport))));
    }
}
