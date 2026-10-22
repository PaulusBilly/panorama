use crate::{
    continue_watching::ContinueCard,
    film_card::{caption, fade_in},
    film_grid::{GridHost, NextCard, PreviousCard, artwork},
    hero,
    home_layout::ViewSettings,
    image_cache::ImageCache,
    routes::home::Home,
    services::Services,
    theme::{
        DURATION_DELIBERATE, EASE_EDITORIAL, SIDEBAR_WIDTH, caps, focus_ring, typography::SECTION,
    },
    transition::Tween,
};
use gpui::{
    App, Context, Div, Entity, FocusHandle, ScrollHandle, Window, div, linear_color_stop,
    linear_gradient, point, prelude::*, px, relative,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// 16:9, fixed so nothing jumps when details arrive.
pub const CARD: (f32, f32) = (360.0, 202.5);
const GAP: f32 = 12.0;
const HEADING_GAP: f32 = 14.0;
/// Room around the cards inside the scroll area, which clips, for the outer focus ring.
const RING: f32 = 4.0;
/// Distance from one card's left edge to the next.
const PITCH: f32 = CARD.0 + GAP;
/// Quiet time after the last wheel event, momentum included, before the row settles.
const SETTLE_AFTER: Duration = Duration::from_millis(140);

/// The Continue row, a carousel in the manner of Apple TV: it runs the full window width, so
/// cards scrolled past stay visible in the gutter and fade toward the sidebar, and whenever the
/// row comes to rest it glides so a card sits exactly on the column's left edge.
#[derive(Default)]
pub struct ContinueRow {
    pub scroll: ScrollHandle,
    pub cards: Vec<FocusHandle>,
    /// When the pointer last scrolled the row; the row settles once it has been quiet.
    wheeled: Option<Instant>,
    /// The glide onto a card, in scroll pixels from the start of the row.
    glide: Option<Tween>,
}

/// Where a row left at `position` comes to rest: the nearest card edge.
fn rest(position: f32) -> f32 {
    (position / PITCH).round() * PITCH
}

impl ContinueRow {
    /// The pointer scrolled the row: let it move freely and settle once it stops.
    pub fn wheel(&mut self) {
        self.wheeled = Some(Instant::now());
        self.glide = None;
    }

    /// Glide so card `index` sits on the column's left edge, or as near as the row allows.
    pub fn show(&mut self, index: usize) {
        self.glide_to(index as f32 * PITCH, Instant::now());
        self.wheeled = None;
    }

    /// Frames are needed while the row waits to settle or glides.
    pub fn moving(&self, reduced: bool) -> bool {
        self.wheeled.is_some()
            || self
                .glide
                .is_some_and(|glide| glide.moving(Instant::now(), reduced))
    }

    fn position(&self) -> f32 {
        -f32::from(self.scroll.offset().x)
    }

    fn glide_to(&mut self, target: f32, now: Instant) {
        let target = target.clamp(0.0, f32::from(self.scroll.max_offset().x).max(0.0));
        let mut glide = Tween::fixed(self.position());
        glide.retarget(target, DURATION_DELIBERATE, Some(EASE_EDITORIAL), now);
        self.glide = Some(glide);
    }

    /// Advance the settle and the glide; called once per frame before layout.
    fn settle(&mut self, reduced: bool) {
        let now = Instant::now();
        if self
            .wheeled
            .is_some_and(|at| now.duration_since(at) >= SETTLE_AFTER)
        {
            self.wheeled = None;
            self.glide_to(rest(self.position()), now);
        }
        if let Some(glide) = self.glide {
            self.scroll
                .set_offset(point(px(-glide.value(now, reduced)), px(0.0)));
            if !glide.moving(now, reduced) {
                self.glide = None;
            }
        }
    }

    pub fn sync(&mut self, len: usize, cx: &mut App) {
        self.cards.resize_with(len, || cx.focus_handle());
    }

    pub fn height() -> f32 {
        SECTION.size * SECTION.line_height + HEADING_GAP + CARD.1
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        cards: &[ContinueCard],
        view: ViewSettings,
        images: &Entity<ImageCache>,
        services: Option<&Arc<Services>>,
        visible: bool,
        window: &Window,
        cx: &mut Context<Home>,
    ) -> Div {
        self.settle(view.reduced);
        let theme = view.theme;
        let col = view.column();
        let position = self.position();
        // Full window width from x = 0; a leading spacer puts the first card on the column.
        let mut row = div()
            .id("continue-cards")
            .flex()
            .gap(px(GAP))
            .py(px(RING))
            .w(px(view.width))
            .overflow_x_scroll()
            // Only sideways gestures move the row, so scrolling down over it moves the page.
            .restrict_scroll_to_axis()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|home, _, _, cx| {
                home.row.wheel();
                cx.notify();
            }))
            .child(div().flex_shrink_0().w(px(col.left - GAP)).h(px(CARD.1)));
        for (index, (card, focus)) in cards.iter().zip(&self.cards).enumerate() {
            let (image, poster, _) =
                artwork(images, services, &card.film, CARD, visible, window, cx);
            let x = col.left + index as f32 * PITCH - position;
            row = row.child(self.card(
                card,
                index,
                image.map(|i| (i, poster)),
                focus,
                x,
                view,
                window,
                cx,
            ));
        }
        // The right gutter, so the last card stops where the column does.
        row = row.child(
            div()
                .flex_shrink_0()
                .w(px((view.width - col.left - col.width - GAP).max(0.0)))
                .h(px(CARD.1)),
        );
        div()
            .flex()
            .flex_col()
            .child(
                SECTION
                    .apply(div())
                    .ml(px(col.left))
                    .mb(px(HEADING_GAP - RING))
                    .text_color(theme.ink)
                    .child(caps("Continue")),
            )
            .child(row.mb(px(-RING)))
    }

    #[allow(clippy::too_many_arguments)]
    fn card(
        &self,
        card: &ContinueCard,
        index: usize,
        image: Option<(crate::image_cache::CachedImage, bool)>,
        focus: &FocusHandle,
        x: f32,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Home>,
    ) -> Div {
        let theme = view.theme;
        let film = card.film.clone();
        let col = view.column();
        let mut tile = div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .bg(theme.artwork_empty);
        if let Some((image, poster)) = image {
            let fade = fade_in(&image, view.reduced);
            tile = tile.child(hero::cover(image.image, CARD, poster, 1.0).opacity(fade));
        }
        let percent = (card.progress * 100.0).round() as u32;
        let button =
            focus_ring(
                div()
                    .id(("continue-card", index))
                    .key_context("PanoramaCard")
                    .on_action(cx.listener(move |home, _: &NextCard, window, cx| {
                        home.step_row(index, true, window, cx)
                    }))
                    .on_action(cx.listener(move |home, _: &PreviousCard, window, cx| {
                        home.step_row(index, false, window, cx)
                    }))
                    .absolute()
                    .inset_0()
                    .cursor_pointer()
                    .role(gpui::Role::Button)
                    .aria_label(format!(
                        "Open details for {}, {percent}% watched",
                        film.name
                    ))
                    .on_click(cx.listener(move |home, _, window, cx| {
                        home.open_film(film.clone(), window, cx)
                    })),
                focus,
                theme,
                view.active,
                view.keyboard,
                0.0,
                window,
            )
            .absolute()
            .inset_0();
        // The grid tile's scrim and caption, with progress as a hairline along the bottom edge.
        let fade = theme.scrim.opacity(0.58);
        let tile = tile
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .h(relative(0.46))
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(fade.opacity(0.0), 0.0),
                        linear_color_stop(fade, 1.0),
                    )),
            )
            .child(caption(&card.film, theme))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(3.0))
                    .bg(theme.progress_track)
                    .child(
                        div()
                            .h_full()
                            .w(relative(card.progress))
                            .bg(theme.progress_fill),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_1()
                    .border_color(theme.image_outline),
            )
            // A card in the gutter fades toward the sidebar. GPUI cannot mask or blur, so the
            // fade is backdrop-coloured: one window-anchored gradient, gone by the column edge.
            .when(x < col.left, |tile| {
                tile.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(-x))
                        .w(px(col.left))
                        .bg(linear_gradient(
                            90.0,
                            linear_color_stop(theme.scrim, SIDEBAR_WIDTH * 0.5 / col.left),
                            linear_color_stop(theme.scrim.opacity(0.0), 1.0),
                        )),
                )
            });
        div()
            .relative()
            .flex_shrink_0()
            .w(px(CARD.0))
            .h(px(CARD.1))
            .child(tile)
            .child(button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_rests_with_the_nearest_card_on_the_column() {
        assert_eq!(rest(0.0), 0.0);
        assert_eq!(rest(PITCH * 0.4), 0.0);
        assert_eq!(rest(PITCH * 0.6), PITCH);
        assert_eq!(rest(PITCH * 2.49), PITCH * 2.0);
    }
}
