use crate::{
    button::{Button, Icon},
    film_card::{self, CardProps},
    home_layout::{GAP, Grid, ViewSettings},
    image_cache::{CachedImage, ImageCache},
    services::Services,
    theme::{Column, Theme, Typography, motion::*},
    transition::Tween,
};
use gpui::{
    App, Context, Div, Entity, FocusHandle, PathBuilder, Pixels, Point, ScrollHandle, Window, div,
    linear_color_stop, linear_gradient, point, prelude::*, px, relative,
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
            (
                false,
                Self::Dwelling {
                    index: dwelling, ..
                },
            ) if dwelling == index => Self::Closed,
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
    /// A line over each card's title, by index; empty for none.
    notes: Arc<[Option<String>]>,
    cards: Vec<FocusHandle>,
    spare: FocusHandle,
    watch: FocusHandle,
    preview: Preview,
    /// A preview that was left, settling back onto its card.
    fading: Option<(usize, Tween)>,
    /// Captures pin the first card's preview; the real pointer must not move it.
    pinned: bool,
    launched: Instant,
}

impl FilmGrid {
    /// An empty grid; `hover_first` opens the first card's preview at once, for captures.
    pub fn new(hover_first: bool, cx: &mut App) -> Self {
        Self {
            films: Arc::from([]),
            notes: Arc::from([]),
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
            fading: None,
            pinned: hover_first,
            launched: Instant::now(),
        }
    }
    /// Replace the films, keeping one focus handle per card.
    pub fn set_films(&mut self, films: Arc<[FilmDetails]>, cx: &mut App) {
        self.cards.resize_with(films.len(), || cx.focus_handle());
        self.films = films;
        self.notes = Arc::from([]);
    }
    /// Replace the films with a note over each title.
    pub fn set_films_with_notes(
        &mut self,
        films: Arc<[FilmDetails]>,
        notes: Arc<[Option<String>]>,
        cx: &mut App,
    ) {
        self.set_films(films, cx);
        self.notes = notes;
    }
    fn note(&self, index: usize) -> Option<&str> {
        self.notes.get(index).and_then(Option::as_deref)
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
        if let Preview::Open {
            index: open,
            mut tween,
        } = self.preview
            && open == index
        {
            tween.retarget(0.0, DURATION_STANDARD, Some(EASE_EDITORIAL), Instant::now());
            self.fading = Some((index, tween));
        }
        self.preview = self.preview.leave(index);
    }
    /// Frames are needed while a dwell counts down or a preview opens or settles.
    pub fn moving(&self, reduced: bool) -> bool {
        let now = Instant::now();
        let fading = self
            .fading
            .is_some_and(|(_, tween)| tween.moving(now, reduced));
        fading
            || match self.preview {
                Preview::Closed => false,
                Preview::Dwelling { .. } => true,
                Preview::Open { tween, .. } => tween.moving(now, reduced),
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
        let notes = self.notes.clone();
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
                        note: notes.get(index).and_then(Option::as_deref),
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
        let now = Instant::now();
        self.preview = self.preview.tick(now);
        if self
            .fading
            .is_some_and(|(_, tween)| !tween.moving(now, view.reduced))
        {
            self.fading = None;
        }
        let rendered = start * grid.columns..end * grid.columns;
        let open = match self.preview {
            Preview::Open { index, tween } => Some((index, tween, true)),
            _ => None,
        };
        for (index, tween, live) in self
            .fading
            .map(|(i, t)| (i, t, false))
            .into_iter()
            .chain(open)
        {
            if let Some(film) = films.get(index)
                && rendered.contains(&index)
            {
                let lift = tween.value(now, view.reduced);
                result = result.child(self.preview(
                    film, index, lift, live, grid, view, images, services, window, cx,
                ));
            }
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
        live: bool,
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
        // The artwork grows upward so its bottom stays on the row's baseline, like the tiles beside it.
        let y = (index / grid.columns) as f32 * grid.stride() - (height - grid.height());
        // Details are laid out once at the open size and pinned there, so the synopsis never
        // rewraps or slides while the card grows; artwork is requested at that size for the same reason.
        let full = grid.width * (1.0 + LIFT);
        let inset = (full * 0.055).clamp(16.0, 28.0);
        let (image, poster, _) = artwork(
            images,
            services,
            film,
            (full, full * 9.0 / 16.0),
            true,
            window,
            cx,
        );

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
        // The tile's own scrim, then, as the card lifts, the artwork melts into the details
        // panel like a ticket stub: a long soft darkening and a short run to solid surface.
        let fade = |share: f32, alpha: f32| {
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .w_full()
                .h(relative(share))
                .bg(linear_gradient(
                    180.0,
                    linear_color_stop(theme.surface.opacity(0.0), 0.0),
                    linear_color_stop(theme.surface.opacity(alpha * lift), 1.0),
                ))
        };
        let scrim = theme.scrim.opacity(0.58);
        let art = art
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .h(relative(0.46))
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(scrim.opacity(0.0), 0.0),
                        linear_color_stop(scrim, 1.0),
                    )),
            )
            .child(fade(0.75, 0.8))
            .child(fade(0.3, 1.0))
            .child(film_card::caption(film, self.note(index), theme))
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
            .w(px(full))
            .ml(px(-(full - width) / 2.0))
            .px(px(inset))
            .pt(px(perforation(full) + 24.0))
            .pb(px(inset + full * WAVE_DEPTH))
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
            .id((
                if live {
                    "film-preview"
                } else {
                    "film-preview-fading"
                },
                index,
            ))
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::black().opacity(0.7 * lift),
                offset: point(px(0.0), px(0.0)),
                blur_radius: px(40.0),
                spread_radius: px(4.0),
                inset: false,
            }])
            .when(live, |preview| {
                // Clicks stop here so the cards beneath stay quiet; the wheel still scrolls the page.
                preview.block_mouse_except_scroll().on_hover(cx.listener(
                    move |host: &mut V, over: &bool, _, cx| {
                        if !*over {
                            host.film_grid().leave_preview(index);
                            cx.notify();
                        }
                    },
                ))
            })
            .child(
                art.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_1()
                        .border_b_0()
                        .border_color(theme.image_outline),
                ),
            )
            .child(stub(height, theme, lift))
            .child(details)
    }
}

/// Ticket proportions measured from A24's AAA24 tickets, as fractions of the card's width.
/// Notch radius: a semicircle bitten out of each side at the perforation.
const NOTCH: f32 = 0.04;
/// Depth of each bite along the bottom edge.
const WAVE_DEPTH: f32 = 0.016;
/// Bite spacing along the bottom edge: this many pitches across the card.
const BITES: usize = 15;
/// Share of each pitch a bite spans; the rest is flat land.
const BITE_CHORD: f32 = 0.78;

/// Perforation line, measured down from the top of the details panel. The notch sits wholly
/// below the artwork, which GPUI cannot cut.
fn perforation(width: f32) -> f32 {
    width * NOTCH + 6.0
}

/// The details panel drawn as a ticket stub under the artwork: side notches on a dashed
/// perforation and a bitten bottom edge, filled with surface and outlined like the tiles.
/// It spans the card's current width, so the shape grows with the lift while the text holds still.
fn stub(top: f32, theme: Theme, lift: f32) -> impl IntoElement {
    let fill = theme.surface.opacity(lift);
    let outline = theme.image_outline.opacity(lift);
    let dash = theme.ink_muted.opacity(0.35 * lift);
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let at = |x: f32, y: f32| bounds.origin + point(px(x), px(y));
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let (notch, depth, line) = (w * NOTCH, w * WAVE_DEPTH, perforation(w));
            // Down the left side, along the bottom, up the right; every bite curves into the
            // panel. `e` insets the path so a 1 px stroke lands inside the card's clip.
            let edge = |path: &mut PathBuilder, e: f32| {
                let (left, right, bottom) = (e, w - e, h - e);
                let bite = |path: &mut PathBuilder, x: f32, from: f32, to: f32| {
                    path.line_to(at(x, from));
                    path.arc_to(point(px(notch), px(notch)), px(0.0), false, true, at(x, to));
                };
                bite(path, left, line - notch, line + notch);
                // Round bites cut up into a straight edge, flat land between them, and half a
                // bite at each corner. Each is a shallow arc of a circle whose centre sits below.
                let pitch = (right - left) / BITES as f32;
                let half = pitch * BITE_CHORD / 2.0;
                let r = (half * half + depth * depth) / (2.0 * depth);
                let arc = |path: &mut PathBuilder, to: Point<Pixels>| {
                    path.arc_to(point(px(r), px(r)), px(0.0), false, true, to);
                };
                path.line_to(at(left, bottom - depth));
                arc(path, at(left + half, bottom));
                for i in 1..BITES {
                    let x = left + i as f32 * pitch;
                    path.line_to(at(x - half, bottom));
                    arc(path, at(x + half, bottom));
                }
                path.line_to(at(right - half, bottom));
                arc(path, at(right, bottom - depth));
                bite(path, right, line + notch, line - notch);
                path.line_to(at(right, 0.0));
            };
            let mut body = PathBuilder::fill();
            body.move_to(at(0.0, 0.0));
            edge(&mut body, 0.0);
            body.close();
            let mut rim = PathBuilder::stroke(px(1.0));
            rim.move_to(at(0.5, 0.0));
            edge(&mut rim, 0.5);
            let mut perforation = PathBuilder::stroke(px(1.0)).dash_array(&[px(4.0), px(4.0)]);
            perforation.move_to(at(notch + 8.0, line));
            perforation.line_to(at(w - notch - 8.0, line));
            for (path, color) in [(body, fill), (rim, outline), (perforation, dash)] {
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .absolute()
    .top(px(top))
    .left_0()
    .right_0()
    .bottom_0()
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
        assert_eq!(
            open.hover(2, false, ms(600)).open(),
            Some(2),
            "card under preview"
        );
        assert_eq!(open.hover(2, true, ms(700)).open(), Some(2));
        assert_eq!(open.leave(3).open(), Some(2));
        assert_eq!(open.leave(2).open(), None);
    }
}
