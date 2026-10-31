//! A card's artwork growing to fill the window, and shrinking back onto its card on the way out.
//!
//! GPUI draws one route at a time, so this overlay runs over whichever page has the grid: the
//! film page mounts once the artwork is where its backdrop will be, and going back returns to
//! the grid at once and lands on it.

use crate::{
    app::AppShell,
    film_grid, hero,
    router::Route,
    theme::{
        DURATION_DELIBERATE, DURATION_STANDARD, Theme,
        motion::EASE_MENU,
        scrim::{Edge, scrim},
    },
    transition::{Tween, bezier},
};
use gpui::{
    Bounds, Context, MouseButton, Pixels, Point, RenderImage, Size, Task, div, point, prelude::*,
    relative,
};
use panorama_core::addons::FilmDetails;
use std::{sync::Arc, time::Duration, time::Instant};

pub const DURATION: Duration = DURATION_DELIBERATE;

/// The card as it looks at the moment of the click.
#[derive(Clone)]
pub struct CardArt {
    /// The film on the card, which a festival lookup may have swapped for its addon match.
    pub film: FilmDetails,
    pub image: Arc<RenderImage>,
    pub poster: bool,
    pub bounds: Bounds<Pixels>,
    /// Where the card sits unhovered, which is where the way back lands.
    pub rest: Bounds<Pixels>,
    pub lift: f32,
    pub note: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Direction {
    Open,
    Close,
}

/// The card's title and fades only show near the card: they are gone early on the way up and
/// come back late on the way down.
const FACE_FILL: f32 = 0.35;

fn face_opacity(fill: f32) -> f32 {
    (1.0 - fill / FACE_FILL).clamp(0.0, 1.0)
}

/// How long a card that was landed on keeps its caption hidden.
const QUIET: Duration = Duration::from_secs(1);

/// The card a film page just shrank onto, whose caption waits and then fades in.
#[derive(Clone)]
pub struct Quiet {
    id: String,
    /// When the caption starts to fade in.
    shows: Instant,
}

impl Quiet {
    pub fn new(id: String) -> Self {
        Self {
            id,
            shows: Instant::now() + DURATION + QUIET,
        }
    }
    pub fn active(&self) -> bool {
        Instant::now() < self.shows + DURATION_STANDARD
    }
    fn opacity_at(&self, id: &str, now: Instant) -> f32 {
        if id != self.id {
            return 1.0;
        }
        let shown = now.saturating_duration_since(self.shows);
        bezier(
            (shown.as_secs_f32() / DURATION_STANDARD.as_secs_f32()).min(1.0),
            EASE_MENU,
        )
    }
}

/// How visible film `id`'s card caption is right now.
pub fn caption_opacity(quiet: Option<&Quiet>, id: &str) -> f32 {
    quiet.map_or(1.0, |quiet| quiet.opacity_at(id, Instant::now()))
}

/// What the way back needs, kept from the click for as long as the card can still be found.
pub struct Returning {
    pub card: CardArt,
    pub film: FilmDetails,
    pub from: Route,
    pub viewport: Size<Pixels>,
}

pub struct Zoom {
    pub card: CardArt,
    pub film: FilmDetails,
    pub direction: Direction,
    /// How much of the window the artwork fills, from 0 on the card to 1 full.
    fill: Tween,
    /// Which card it hangs from, from 0 lifted to 1 at rest, so a reversal has no jump.
    rested: Tween,
    /// Opens the film page, or clears the overlay, when the artwork lands.
    _done: Task<()>,
}

impl Zoom {
    pub fn new(card: CardArt, film: FilmDetails, direction: Direction, done: Task<()>) -> Self {
        let start = match direction {
            Direction::Open => 0.0,
            Direction::Close => 1.0,
        };
        let mut zoom = Self {
            card,
            film,
            direction: Direction::Close,
            fill: Tween::fixed(start),
            rested: Tween::fixed(start),
            _done: Task::ready(()),
        };
        zoom.turn(direction, done);
        zoom
    }

    /// Head the other way from wherever the artwork is now.
    pub fn turn(&mut self, direction: Direction, done: Task<()>) {
        let now = Instant::now();
        let (fill, rested) = match direction {
            Direction::Open => (1.0, 0.0),
            Direction::Close => (0.0, 1.0),
        };
        self.fill.retarget(fill, DURATION, Some(EASE_MENU), now);
        self.rested.retarget(rested, DURATION, Some(EASE_MENU), now);
        self.direction = direction;
        self._done = done;
    }

    pub fn render(
        &self,
        viewport: Size<Pixels>,
        theme: Theme,
        cx: &mut Context<AppShell>,
    ) -> impl IntoElement {
        let now = Instant::now();
        let fill = self.fill.value(now, false);
        let rested = self.rested.value(now, false);
        let card = between(self.card.bounds, self.card.rest, rested);
        let rect = between(card, Bounds::new(Point::default(), viewport), fill);
        let size = (f32::from(rect.size.width), f32::from(rect.size.height));
        let art = hero::cover(self.card.image.clone(), size, self.card.poster, 1.0);
        let lift = self.card.lift * (1.0 - rested);
        let face = film_grid::card_face(
            &self.card.film,
            self.card.note.as_deref(),
            lift,
            1.0 - rested,
            theme,
        );
        let closing = self.direction == Direction::Close;
        div().absolute().inset_0().occlude().child(
            div()
                .id("zoom")
                .absolute()
                .left(rect.origin.x)
                .top(rect.origin.y)
                .w(rect.size.width)
                .h(rect.size.height)
                .overflow_hidden()
                .when(closing, |zoom| {
                    zoom.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|shell, _, window, cx| shell.reopen(window, cx)),
                    )
                })
                .child(art)
                // The film page's scrims, there as soon as the artwork is the page's backdrop.
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .opacity(fill)
                        .child(scrim(theme, Edge::Top, relative(0.3), 0.6))
                        .child(scrim(theme, Edge::Bottom, relative(0.45), 1.0)),
                )
                .child(face.opacity(face_opacity(fill))),
        )
    }
}

fn between(from: Bounds<Pixels>, to: Bounds<Pixels>, t: f32) -> Bounds<Pixels> {
    let mix = |a: Pixels, b: Pixels| a + (b - a) * t;
    Bounds::new(
        point(
            mix(from.origin.x, to.origin.x),
            mix(from.origin.y, to.origin.y),
        ),
        gpui::size(
            mix(from.size.width, to.size.width),
            mix(from.size.height, to.size.height),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, size};

    #[test]
    fn a_landed_on_card_holds_its_caption_then_fades_it_in() {
        let quiet = Quiet::new("tt1".into());
        assert_eq!(quiet.opacity_at("tt2", Instant::now()), 1.0);
        let landed = quiet.shows - QUIET;
        assert_eq!(quiet.opacity_at("tt1", landed), 0.0);
        assert_eq!(quiet.opacity_at("tt1", quiet.shows), 0.0);
        assert_eq!(
            quiet.opacity_at("tt1", quiet.shows + DURATION_STANDARD),
            1.0
        );
        assert_eq!(caption_opacity(None, "tt1"), 1.0);
    }
    #[test]
    fn the_title_only_shows_near_the_card() {
        assert_eq!(face_opacity(0.0), 1.0);
        assert!((face_opacity(FACE_FILL / 2.0) - 0.5).abs() < 0.001);
        assert_eq!(face_opacity(FACE_FILL), 0.0);
        assert_eq!(face_opacity(1.0), 0.0);
    }
    #[test]
    fn the_card_grows_to_exactly_the_window() {
        let card = Bounds::new(point(px(300.0), px(200.0)), size(px(400.0), px(200.0)));
        let window = Bounds::new(point(px(0.0), px(0.0)), size(px(1000.0), px(800.0)));
        assert_eq!(between(card, window, 0.0), card);
        assert_eq!(
            between(card, window, 0.5),
            Bounds::new(point(px(150.0), px(100.0)), size(px(700.0), px(500.0)))
        );
        assert_eq!(between(card, window, 1.0), window);
    }
}
