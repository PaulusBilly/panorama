//! Home's Continue row: films in progress after the hero's, as landscape cards with progress.
use crate::{
    continue_watching::ContinueCard,
    film_card::fade_in,
    film_grid::{GridHost, NextCard, PreviousCard, artwork},
    hero,
    home_layout::ViewSettings,
    image_cache::ImageCache,
    routes::home::Home,
    services::Services,
    theme::{
        caps, credits, focus_ring,
        scrim::{Edge, scrim},
        typography::{CREDIT, SECTION},
    },
};
use gpui::{
    App, Context, Div, Entity, FocusHandle, ScrollHandle, Window, div, prelude::*, px, relative,
};
use std::sync::Arc;

/// 16:9, fixed so nothing jumps when details arrive.
pub const CARD: (f32, f32) = (300.0, 168.75);
/// Between cards.
const GAP: f32 = 12.0;
/// Heading to cards.
const HEADING_GAP: f32 = 14.0;
/// Room around the cards inside the scroll area, which clips, for the outer focus ring.
const RING: f32 = 4.0;

/// Horizontal scroll and card focus, per Home entry.
#[derive(Default)]
pub struct ContinueRow {
    pub scroll: ScrollHandle,
    pub cards: Vec<FocusHandle>,
}

impl ContinueRow {
    /// Keep one focus handle per card.
    pub fn sync(&mut self, len: usize, cx: &mut App) {
        self.cards.truncate(len);
        while self.cards.len() < len {
            self.cards.push(cx.focus_handle());
        }
    }

    /// Heading plus cards.
    pub fn height() -> f32 {
        SECTION.size * SECTION.line_height + HEADING_GAP + CARD.1
    }

    /// "CONTINUE" at the column's left edge, then a row that starts there and bleeds to the
    /// window's right edge, scrolling sideways when it overflows.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        cards: &[ContinueCard],
        view: ViewSettings,
        images: &Entity<ImageCache>,
        services: Option<&Arc<Services>>,
        visible: bool,
        window: &Window,
        cx: &mut Context<Home>,
    ) -> Div {
        let theme = view.theme;
        let col = view.column();
        let mut row = div()
            .id("continue-cards")
            .flex()
            .gap(px(GAP))
            .p(px(RING))
            .w(px(view.width - col.left + RING))
            .overflow_x_scroll()
            .track_scroll(&self.scroll);
        for (index, card) in cards.iter().enumerate() {
            let Some(focus) = self.cards.get(index) else {
                break;
            };
            let (image, poster, _) =
                artwork(images, services, &card.film, CARD, visible, window, cx);
            row = row.child(self.card(
                card,
                index,
                image.map(|i| (i, poster)),
                focus,
                view,
                window,
                cx,
            ));
        }
        // The right gutter, so the last card stops where the column does.
        row = row.child(
            div()
                .flex_shrink_0()
                .w(px((view.width - col.left - col.width - GAP - RING).max(0.0)))
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
            .child(row.ml(px(col.left - RING)).mb(px(-RING)))
    }

    #[allow(clippy::too_many_arguments)]
    fn card(
        &self,
        card: &ContinueCard,
        index: usize,
        image: Option<(crate::image_cache::CachedImage, bool)>,
        focus: &FocusHandle,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Home>,
    ) -> Div {
        let theme = view.theme;
        let film = card.film.clone();
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
                    .hover(|style| style.bg(theme.hover))
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
        let tile = tile
            .child(scrim(theme, Edge::Bottom, relative(0.7), 0.94))
            .child(
                div()
                    .absolute()
                    .left(px(16.0))
                    .right(px(16.0))
                    .bottom(px(14.0))
                    .flex()
                    .flex_col()
                    .text_color(theme.ink)
                    .child(SECTION.apply(div()).truncate().child(caps(&card.film.name)))
                    .child(
                        credits(&card.film, theme)
                            .min_h(px(CREDIT.size * CREDIT.line_height))
                            .mt(px(2.0)),
                    )
                    .child(
                        div()
                            .mt(px(12.0))
                            .h(px(3.0))
                            .w_full()
                            .bg(theme.progress_track)
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(card.progress.clamp(0.0, 1.0)))
                                    .bg(theme.progress_fill),
                            ),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_1()
                    .border_color(theme.image_outline),
            );
        div()
            .relative()
            .flex_shrink_0()
            .w(px(CARD.0))
            .h(px(CARD.1))
            .child(tile)
            .child(button)
    }
}
