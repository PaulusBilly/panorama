use super::*;
use crate::assets::Weighted;
use crate::{
    film_card,
    film_display::{Quality, rating, rating_count},
    hero,
    theme::{
        DURATION_STANDARD, EASE_EDITORIAL, SHIMMER, Typography, caps,
        scrim::{Edge, scrim},
        typography::{BODY, DISPLAY, LABEL},
    },
    titlebar::SAFE_TOP,
    transition::bezier,
};
use gpui::{Div, FontWeight, Render, div, img, point, prelude::*, px, relative, svg};

impl Film {
    fn metadata_content(
        &mut self,
        meta: &FilmDetails,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let logo = meta
            .logo
            .as_ref()
            .map(|url| self.image(url, (310.0, 128.0), view, window, cx));
        let mut title = div();
        if let Some((Some(logo), false)) = logo.as_ref() {
            let size = logo.image.size(0);
            let ratio = (310.0 / size.width.0 as f32)
                .min(128.0 / size.height.0 as f32)
                .min(1.0 / window.scale_factor());
            title = title.child(
                img(logo.image.clone())
                    .w(px(size.width.0 as f32 * ratio))
                    .h(px(size.height.0 as f32 * ratio)),
            );
        } else if logo.is_none() || logo.as_ref().is_some_and(|(_, failed)| *failed) {
            title = title.child(
                DISPLAY
                    .apply(div())
                    .text_size(px((view.width * 0.05).clamp(36.0, DISPLAY.size)))
                    .child(caps(&meta.name)),
            );
        }
        let quality = self
            .selected
            .as_ref()
            .and_then(|source| match &self.groups.get(source.group)?.state {
                panorama_core::addons::StreamState::Ready(streams) => streams.get(source.stream),
                _ => None,
            })
            .map(Quality::from_source)
            .unwrap_or_default();
        let mut column = div()
            .w_full()
            .max_w(px(640.0))
            .flex()
            .flex_col()
            .text_color(theme.ink)
            .child(title);
        if !meta.director.is_empty() || meta.origin_country.is_some() || meta.release_info.is_some()
        {
            column = column.child(crate::theme::credits(meta, theme).mt(px(16.0)));
        }
        let votes = self.args.fixtures.then_some(match self.id.as_str() {
            "tmdb:101" | "tmdb:104" | "tmdb:107" => 1824,
            "tmdb:102" | "tmdb:105" | "tmdb:108" => 3201,
            _ => 2485,
        });
        let mut facts = LABEL
            .apply(div())
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(16.0))
            .gap_y(px(8.0))
            .text_color(theme.ink_muted)
            .font_features(gpui::FontFeatures(Arc::new(vec![
                ("tnum".into(), 1),
                ("kern".into(), 1),
            ])));
        let mut any = false;
        if let Some(score) = rating(meta.imdb_rating.as_deref(), votes) {
            any = true;
            facts = facts.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        svg()
                            .path("star-filled.svg")
                            .size(px(14.0))
                            .text_color(theme.ink),
                    )
                    .child(div().text_color(theme.ink).child(score))
                    .when_some(votes, |row, votes| row.child(rating_count(votes))),
            );
        }
        if !meta.genres.is_empty() {
            any = true;
            facts = facts.child(meta.genres.join(", "));
        }
        if let Some(runtime) = &meta.runtime {
            any = true;
            facts = facts.child(runtime.clone());
        }
        for badge in quality
            .video
            .into_iter()
            .chain(quality.surround.then_some("5.1"))
        {
            any = true;
            facts = facts.child(
                div()
                    .border_1()
                    .border_color(theme.ink_muted)
                    .px(px(6.0))
                    .py(px(1.0))
                    .text_size(px(11.0))
                    .weight(FontWeight::BOLD)
                    .line_height(relative(1.4))
                    .text_color(theme.ink)
                    .child(badge),
            );
        }
        if any {
            column = column.child(facts.mt(px(12.0)));
        }
        column = column.child(
            BODY.apply(div())
                .mt(px(20.0))
                .text_size(px(15.0))
                .text_color(theme.ink)
                .child(
                    meta.description
                        .clone()
                        .filter(|text| !text.trim().is_empty())
                        .unwrap_or_else(|| "No synopsis is available for this film.".into()),
                ),
        );
        column = column.child(self.actions(view, cx).mt(px(28.0)));
        if matches!(self.account, Account::SignedIn(_))
            && !self.sources_loading
            && self.selected.is_none()
        {
            column = column.child(
                LABEL
                    .apply(div())
                    .id("film-source-status")
                    .role(gpui::Role::Status)
                    .mt(px(20.0))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(16.0))
                    .text_color(theme.ink_muted)
                    .child(if self.sources_error {
                        "Sources could not be loaded."
                    } else {
                        "No playable source is available from your addons."
                    })
                    .child(self.retry(false, view, cx)),
            );
        }
        if self.watchlist_error {
            column = column.child(
                LABEL
                    .apply(div())
                    .id("film-watchlist-error")
                    .role(gpui::Role::Alert)
                    .mt(px(12.0))
                    .text_color(theme.danger)
                    .child("Could not update your Watchlist. Try again."),
            );
        }
        column
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = self.settings(window, cx);
        if view.active
            && self.debug_step < self.args.scroll.len()
            && self.launched.elapsed().as_secs_f32() > 0.25 + 0.2 * self.debug_step as f32
        {
            let top = -f32::from(self.scroll.offset().y);
            let max = f32::from(self.scroll.max_offset().y);
            let top = (top + self.args.scroll[self.debug_step]).min(max).max(0.0);
            self.scroll.set_offset(point(px(0.0), px(-top)));
            self.debug_step += 1;
        }
        cx.notify();
    }
}
impl Render for Film {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.settings(window, cx);
        if self.autoplay
            && (self.selected.is_some() || !matches!(self.account, Account::SignedIn(_)))
        {
            self.autoplay = false;
            cx.defer_in(window, |film, window, cx| {
                film.primary_action(false, true, window, cx)
            });
        }
        let theme = view.theme;
        let col = view.column();
        if view.active {
            self.images.update(cx, |images, _| images.begin_frame());
        }
        // The card's preview metadata seeds the page, but its artwork differs from the details',
        // so nothing paints until the details, backdrop and logo are all in; then it fades in once.
        let (artwork, poster, art_pending) = if self.loading && self.revealed.is_none() {
            (None, false, true)
        } else {
            self.artwork(view, window, cx)
        };
        if self.revealed.is_none() {
            let logo = self.metadata.as_ref().and_then(|meta| meta.logo.clone());
            let logo_pending = logo.is_some_and(|url| {
                matches!(
                    self.image(&url, (310.0, 128.0), view, window, cx),
                    (None, false)
                )
            });
            if view.active && !art_pending && !logo_pending {
                self.revealed = Some(Instant::now());
            }
        }
        let reveal = match self.revealed {
            Some(at) if !view.reduced => bezier(
                (at.elapsed().as_secs_f32() / DURATION_STANDARD.as_secs_f32()).min(1.0),
                EASE_EDITORIAL,
            ),
            Some(_) => 1.0,
            None => 0.0,
        };
        let mut page = div()
            .relative()
            .w_full()
            .min_h(px(view.height))
            .bg(theme.canvas)
            .text_color(theme.ink);
        // The still fills the first viewport only; the bottom scrim lands on canvas, so a page
        // taller than the window continues without a seam.
        if let Some(art) = artwork.filter(|_| self.revealed.is_some()) {
            page = page.child(
                div()
                    .absolute()
                    .opacity(reveal)
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(view.height))
                    .bg(theme.artwork_empty)
                    .child(hero::cover(
                        art.image,
                        (view.width, view.height),
                        poster,
                        1.0,
                    ))
                    .child(scrim(theme, Edge::Top, relative(0.3), 0.6))
                    .child(scrim(theme, Edge::Left, relative(0.65), 0.92))
                    .child(scrim(theme, Edge::Bottom, relative(0.45), 1.0)),
            );
        }
        let metadata = self.metadata.clone();
        let narrow = view.width <= 700.0;
        let mut content = div()
            .relative()
            .w(px(col.width))
            .ml(px(col.left))
            .min_h(px(view.height))
            .pt(px(SAFE_TOP + 72.0))
            .pb(px(if narrow { 40.0 } else { 64.0 }))
            .flex()
            .flex_col()
            .justify_end()
            .child(
                div()
                    .absolute()
                    .top(px(SAFE_TOP + 2.0))
                    .left_0()
                    .child(self.back(view, cx)),
            );
        if self.details_error {
            content = content.child(
                div()
                    .w_full()
                    .max_w(px(520.0))
                    .bg(theme.surface)
                    .px(px(24.0))
                    .py(px(20.0))
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(16.0))
                    .child(
                        div()
                            .id("film-details-error")
                            .role(gpui::Role::Alert)
                            .body()
                            .child(
                                "Unable to load film details. Check your connection and try again.",
                            ),
                    )
                    .child(self.retry(true, view, cx)),
            );
        } else if let Some(meta) = metadata.as_ref().filter(|_| self.revealed.is_some()) {
            content = content.child(
                self.metadata_content(meta, view, window, cx)
                    .opacity(reveal),
            );
        } else {
            let phase = if view.reduced {
                0.0
            } else {
                (self.launched.elapsed().as_secs_f32() / SHIMMER.duration.as_secs_f32()) % 1.0
            };
            let bar = |width: f32, height: f32| {
                div()
                    .relative()
                    .w(px(width))
                    .max_w_full()
                    .h(px(height))
                    .overflow_hidden()
                    .bg(theme.surface_strong)
                    .child(film_card::shimmer(view, phase))
            };
            content = content.child(
                div()
                    .id("film-loading")
                    .role(gpui::Role::Status)
                    .aria_label("Loading film metadata")
                    .w_full()
                    .max_w(px(640.0))
                    .flex()
                    .flex_col()
                    .child(bar(420.0, 44.0))
                    .child(bar(220.0, 14.0).mt(px(20.0)))
                    .child(bar(560.0, 14.0).mt(px(28.0)))
                    .child(bar(380.0, 14.0).mt(px(10.0)))
                    .child(bar(176.0, 52.0).mt(px(32.0))),
            );
        }
        page = page.child(content);
        if view.active {
            self.images.update(cx, |images, _| images.finish_frame());
        }
        if self.sources_loading && !view.reduced
            || reveal < 1.0 && !view.reduced
            || view.active && self.images.read(cx).moving()
            || self.debug_step < self.args.scroll.len()
        {
            cx.on_next_frame(window, |film, window, cx| film.tick(window, cx));
            window.request_animation_frame();
        }
        div()
            .id("film-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|_, _, window, cx| {
                cx.on_next_frame(window, |film, window, cx| film.tick(window, cx));
                // request_animation_frame panics outside render; notify schedules the frame instead.
                cx.notify();
            }))
            .child(page)
    }
}
