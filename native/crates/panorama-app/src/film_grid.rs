use crate::{
    button::{Button, Icon},
    film_card::{self, CardProps},
    home_layout::{GAP, Grid, ViewSettings},
    image_cache::{CachedImage, ImageCache},
    services::Services,
    theme::{Column, Typography, motion::*},
    transition::Tween,
};
use gpui::{
    App, Context, Div, Entity, FocusHandle, ScrollHandle, Window, div, linear_color_stop,
    linear_gradient, point, prelude::*, px, relative,
};
use panorama_core::addons::FilmDetails;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

gpui::actions!(panorama_card, [NextCard, PreviousCard]);

const SKELETONS: usize = 9;
/// How long the pointer rests on a card before its preview opens.
const DWELL: Duration = Duration::from_millis(500);
/// The preview's artwork grows by this fraction of the tile.
const LIFT: f32 = 0.08;

/// The hover preview: resting on a card for `DWELL` opens a larger card with its details.
#[derive(Clone, Copy)]
enum Preview {
    Closed,
    Dwelling { index: usize, since: Instant },
    Open { index: usize, tween: Tween },
}

impl Preview {
    fn hover(self, index: usize, over: bool, now: Instant) -> Self {
        match (over, self) {
            (true, Self::Open { index: open, .. }) if open == index => self,
            (true, _) => Self::Dwelling { index, since: now },
            (false, Self::Dwelling { index: dwelling, .. }) if dwelling == index => Self::Closed,
            (false, _) => self,
        }
    }
    fn leave(self, index: usize) -> Self {
        match self {
            Self::Open { index: open, .. } if open == index => Self::Closed,
            _ => self,
        }
    }
    fn tick(self, now: Instant) -> Self {
        match self {
            Self::Dwelling { index, since } if now.duration_since(since) >= DWELL => {
                let mut tween = Tween::fixed(0.0);
                tween.retarget(1.0, DURATION_STANDARD, Some(EASE_EDITORIAL), now);
                Self::Open { index, tween }
            }
            _ => self,
        }
    }
    #[cfg(test)]
    fn open(self) -> Option<usize> {
        match self {
            Self::Open { index, .. } => Some(index),
            _ => None,
        }
    }
}

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
    /// Open the film and let it start playback.
    fn play_film(&mut self, film: FilmDetails, window: &mut Window, cx: &mut Context<Self>);
}

/// Virtualised film-card grid with focus handles and the hover preview. Owned per entity.
pub struct FilmGrid {
    pub films: Arc<[FilmDetails]>,
    cards: Vec<FocusHandle>,
    spare: FocusHandle,
    watch: FocusHandle,
    preview: Preview,
    /// Captures pin the first card's preview; the real pointer must not move it.
    pinned: bool,
    launched: Instant,
}

impl FilmGrid {
    /// An empty grid; `hover_first` opens the first card's preview at once, for captures.
    pub fn new(hover_first: bool, cx: &mut App) -> Self {
        Self {
            films: Arc::from([]),
            cards: Vec::new(),
            spare: cx.focus_handle(),
            watch: cx.focus_handle(),
            preview: if hover_first {
                Preview::Open {
                    index: 0,
                    tween: Tween::fixed(1.0),
                }
            } else {
                Preview::Closed
            },
            pinned: hover_first,
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
    /// The pointer entered or left card `index`. Entering starts the dwell; leaving a card only
    /// cancels its own dwell, since an open preview covers the card and closes on its own leave.
    pub fn hover(&mut self, index: usize, over: bool) {
        if self.pinned {
            return;
        }
        self.preview = self.preview.hover(index, over, Instant::now());
    }
    fn leave_preview(&mut self, index: usize) {
        if self.pinned {
            return;
        }
        self.preview = self.preview.leave(index);
    }
    /// Frames are needed while a dwell counts down or the preview opens.
    pub fn moving(&self, reduced: bool) -> bool {
        match self.preview {
            Preview::Closed => false,
            Preview::Dwelling { .. } => true,
            Preview::Open { tween, .. } => tween.moving(Instant::now(), reduced),
        }
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
        let height =
            (rows as f32 * grid.stride() - if rows > 0 { grid.row_gap() } else { 0.0 }).max(0.0);
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
                tiles = tiles.child(film_card::render(
                    CardProps {
                        film,
                        image,
                        poster,
                        unavailable,
                        focus: self.cards.get(index).unwrap_or(&self.spare),
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
        self.preview = self.preview.tick(Instant::now());
        if let Preview::Open { index, tween } = self.preview
            && let Some(film) = films.get(index)
            && (start * grid.columns..end * grid.columns).contains(&index)
        {
            let lift = tween.value(Instant::now(), view.reduced);
            result = result
                .child(self.preview(film, index, lift, grid, view, images, services, window, cx));
        }
        result
    }

    /// The opened card: artwork lifted over its neighbours, then Watch, synopsis and facts.
    #[allow(clippy::too_many_arguments)]
    fn preview<V: GridHost>(
        &self,
        film: &FilmDetails,
        index: usize,
        lift: f32,
        grid: Grid,
        view: ViewSettings,
        images: &Entity<ImageCache>,
        services: Option<&Arc<Services>>,
        window: &Window,
        cx: &mut Context<V>,
    ) -> gpui::Stateful<Div> {
        let theme = view.theme;
        let width = grid.width * (1.0 + LIFT * lift);
        let height = width * 9.0 / 16.0;
        let x = (index % grid.columns) as f32 * (grid.width + GAP) - (width - grid.width) / 2.0;
        let y = (index / grid.columns) as f32 * grid.stride() - (height - grid.height()) / 2.0;
        let inset = (width * 0.055).clamp(16.0, 28.0);
        let (image, poster, _) = artwork(images, services, film, (width, height), true, window, cx);

        let mut art = div()
            .id(("film-preview-art", index))
            .relative()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(theme.surface)
            .cursor_pointer();
        if let Some(image) = image {
            art = art.child(crate::hero::cover(
                image.image,
                (width, height),
                poster,
                1.0,
            ));
        }
        let open = film.clone();
        let art = art
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .h(relative(0.5))
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(theme.scrim.opacity(0.0), 0.0),
                        linear_color_stop(theme.scrim.opacity(0.7), 1.0),
                    )),
            )
            .child(film_card::caption(film, width, theme))
            .on_click(cx.listener(move |host: &mut V, _, window, cx| {
                host.open_film(open.clone(), window, cx)
            }));

        let play = film.clone();
        let facts = [
            film.runtime.clone(),
            film.genres.first().cloned(),
            crate::film_display::rating(film.imdb_rating.as_deref(), None)
                .map(|score| format!("IMDb {score}")),
        ];
        let details = div()
            .px(px(inset))
            .pt(px(inset))
            .pb(px(inset))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .opacity(lift)
            .child(
                div().flex().child(
                    Button::primary(("preview-watch", index), "Watch")
                        .leading(Icon::Play)
                        .compact()
                        .aria_label(format!("Watch {}", film.name))
                        .focus(&self.watch, view.keyboard)
                        .on_click(cx.listener(move |host: &mut V, _, window, cx| {
                            host.play_film(play.clone(), window, cx)
                        })),
                ),
            )
            .when_some(film.description.clone(), |details, text| {
                details.child(
                    div()
                        .body()
                        .text_color(theme.ink_muted)
                        .line_clamp(4)
                        .child(text),
                )
            })
            .when(facts.iter().any(Option::is_some), |details| {
                details.child(
                    div()
                        .label()
                        .text_color(theme.ink_muted)
                        .flex()
                        .gap(px(16.0))
                        .children(facts.into_iter().flatten()),
                )
            });

        div()
            .id(("film-preview", index))
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .bg(theme.surface)
            .shadow_lg()
            .occlude()
            .on_hover(cx.listener(move |host: &mut V, over: &bool, _, cx| {
                if !*over {
                    host.film_grid().leave_preview(index);
                    cx.notify();
                }
            }))
            .child(art)
            .child(details)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_opens_only_after_resting_on_one_card() {
        let start = Instant::now();
        let ms = |n| start + Duration::from_millis(n);

        let resting = Preview::Closed.hover(2, true, start);
        assert_eq!(resting.tick(ms(499)).open(), None);
        assert_eq!(resting.tick(ms(500)).open(), Some(2));

        let passed_over = resting.hover(2, false, ms(300)).tick(ms(2000));
        assert_eq!(passed_over.open(), None);

        let moved_on = resting.hover(3, true, ms(300));
        assert_eq!(moved_on.tick(ms(700)).open(), None);
        assert_eq!(moved_on.tick(ms(800)).open(), Some(3));

        let open = resting.tick(ms(500));
        assert_eq!(open.hover(2, false, ms(600)).open(), Some(2), "card under preview");
        assert_eq!(open.hover(2, true, ms(700)).open(), Some(2));
        assert_eq!(open.leave(3).open(), Some(2));
        assert_eq!(open.leave(2).open(), None);
    }
}
