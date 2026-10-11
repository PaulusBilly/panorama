use super::*;
use crate::{
    film_display::{Quality, rating, rating_count},
    hero,
    theme::{Typography, content_width},
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
                div()
                    .text_size(px(32.0))
                    .font_weight(FontWeight::MEDIUM)
                    .line_height(relative(0.9))
                    .child(meta.name.to_uppercase()),
            );
        }
        let quality = self
            .selected
            .as_ref()
            .map(Quality::from_source)
            .unwrap_or_default();
        let mut left = div()
            .w(px(310.0))
            .max_w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .text_size(px(13.0))
            .line_height(relative(1.5))
            .text_color(theme.inverse.opacity(0.85));
        if !meta.director.is_empty() || meta.origin_country.is_some() || meta.release_info.is_some()
        {
            left = left.child(hero::credits(meta, theme).text_color(theme.inverse));
        }
        if !meta.genres.is_empty() {
            left = left.child(div().child(meta.genres.join(", ")));
        }
        let mut badges = div().flex().items_center().gap(px(12.0));
        for badge in quality
            .video
            .into_iter()
            .chain(quality.surround.then_some("5.1"))
        {
            badges = badges.child(
                div()
                    .border_1()
                    .border_color(theme.inverse.opacity(0.65))
                    .px(px(6.0))
                    .py(px(2.0))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::BOLD)
                    .line_height(relative(1.5))
                    .child(badge),
            );
        }
        if let Some(runtime) = &meta.runtime {
            badges = badges.child(
                div()
                    .font_features(gpui::FontFeatures(Arc::new(vec![
                        ("tnum".into(), 1),
                        ("kern".into(), 1),
                    ])))
                    .child(runtime.clone()),
            );
        }
        if quality.video.is_some() || quality.surround || meta.runtime.is_some() {
            left = left.child(badges);
        }
        left = left.child(self.actions(view, window, cx));
        if matches!(self.account, Account::SignedIn(_))
            && !self.sources_loading
            && self.selected.is_none()
        {
            left = left.child(
                div()
                    .id("film-source-status")
                    .role(gpui::Role::Status)
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(12.0))
                    .child(if self.sources_error {
                        "Sources could not be loaded."
                    } else {
                        "No playable source is available from your addons."
                    })
                    .child(self.retry(false, view, window, cx)),
            );
        }
        if self.watchlist_error {
            left = left.child(
                div()
                    .id("film-watchlist-error")
                    .role(gpui::Role::Alert)
                    .child("Could not update your Watchlist. Try again."),
            );
        }
        let synopsis = div()
            .flex_1()
            .min_w_0()
            .max_w(px(650.0))
            .child(
                div()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::BOLD)
                    .line_height(relative(1.0))
                    .child("SYNOPSIS"),
            )
            .child(
                div()
                    .mt(px(8.0))
                    .text_size(px(15.0))
                    .line_height(relative(1.48))
                    .text_color(theme.inverse.opacity(0.95))
                    .child(
                        meta.description
                            .clone()
                            .filter(|text| !text.trim().is_empty())
                            .unwrap_or_else(|| "No synopsis is available for this film.".into()),
                    ),
            );
        div().child(title).child(
            div()
                .mt(px(20.0))
                .flex()
                .items_start()
                .gap(px(70.0))
                .when(view.width <= 900.0, |row| row.flex_col().gap(px(24.0)))
                .child(left)
                .child(synopsis),
        )
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
        if view.active {
            let top = -f32::from(self.scroll.offset().y);
            let ready = self.metadata.is_some();
            let _ = self.shell.update(cx, |shell, cx| {
                shell.update_home_header(self.entry, top, view.height, ready, cx)
            });
        }
        cx.notify();
    }
}
impl Render for Film {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.settings(window, cx);
        let theme = view.theme;
        self.ensure_overlay(view, window, cx);
        if view.active {
            self.images.update(cx, |images, _| images.begin_frame());
        }
        let (artwork, poster) = self.artwork(view, window, cx);
        let mut page = div()
            .relative()
            .w_full()
            .min_h(px(view.height))
            .bg(if self.metadata.is_none() && self.loading {
                theme.surface
            } else {
                theme.artwork_empty
            })
            .text_color(theme.inverse);
        if let Some(art) = artwork {
            page = page.child(hero::cover(
                art.image,
                (view.width, view.height),
                poster,
                1.0,
            ));
        }
        if let Some(overlay) = &self.overlay {
            page = page.child(img(overlay.clone()).absolute().inset_0().size_full());
        }
        let metadata = self.metadata.clone();
        if let Some(meta) = &metadata {
            let votes = self.args.fixtures.then_some(match self.id.as_str() {
                "tmdb:101" | "tmdb:104" | "tmdb:107" => 1824,
                "tmdb:102" | "tmdb:105" | "tmdb:108" => 3201,
                _ => 2485,
            });
            if let Some(score) = rating(meta.imdb_rating.as_deref(), votes) {
                let mut rating = div()
                    .absolute()
                    .top(px(if view.width <= 700.0 { 80.0 } else { 96.0 }))
                    .left(px((view.width - content_width(view.width)) * 0.5))
                    .w(px(content_width(view.width)))
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(4.0))
                            .line_height(relative(1.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child(
                                svg()
                                    .path("star-filled.svg")
                                    .size(px(19.0))
                                    .text_color(theme.inverse),
                            )
                            .child(div().text_size(px(22.0)).child(score))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.inverse.opacity(0.8))
                                    .child("/10"),
                            ),
                    );
                if let Some(votes) = votes {
                    rating = rating.child(
                        div()
                            .mt(px(4.0))
                            .text_size(px(12.0))
                            .line_height(relative(1.5))
                            .text_color(theme.inverse.opacity(0.75))
                            .font_features(gpui::FontFeatures(Arc::new(vec![
                                ("tnum".into(), 1),
                                ("kern".into(), 1),
                            ])))
                            .child(rating_count(votes)),
                    );
                }
                page = page.child(rating);
            }
        }
        let mut content = div()
            .relative()
            .w(px(content_width(view.width)))
            .mx_auto()
            .min_h(px(view.height))
            .pt(px(if view.width <= 700.0 {
                176.0
            } else {
                (view.height * 0.12).clamp(112.0, 144.0)
            }))
            .pb(px(40.0))
            .flex()
            .flex_col()
            .justify_end();
        if self.details_error {
            content = content.child(
                div()
                    .max_w(px(512.0))
                    .bg(theme.player_canvas.opacity(0.7))
                    .p(px(20.0))
                    .body()
                    .child("Unable to load film details. Check your connection and try again.")
                    .child(
                        div()
                            .mt(px(16.0))
                            .text_size(px(14.0))
                            .child(self.retry(true, view, window, cx)),
                    ),
            );
        } else if let Some(meta) = &metadata {
            content = content.child(self.metadata_content(meta, view, window, cx));
        } else {
            content = content.child(
                div()
                    .id("film-loading")
                    .role(gpui::Role::Status)
                    .aria_label("Loading film metadata")
                    .w_full()
                    .max_w(px(576.0))
                    .child(
                        div()
                            .h(px(40.0))
                            .w(relative(2.0 / 3.0))
                            .bg(theme.inverse.opacity(0.2)),
                    )
                    .child(
                        div()
                            .mt(px(16.0))
                            .h(px(16.0))
                            .w(relative(1.0 / 3.0))
                            .bg(theme.inverse.opacity(0.15)),
                    )
                    .child(
                        div()
                            .mt(px(40.0))
                            .h(px(80.0))
                            .w_full()
                            .bg(theme.inverse.opacity(0.1)),
                    ),
            );
        }
        page = page.child(content);
        if view.active {
            self.images.update(cx, |images, _| images.finish_frame());
            let top = -f32::from(self.scroll.offset().y);
            let _ = self.shell.update(cx, |shell, cx| {
                shell.update_home_header(self.entry, top, view.height, metadata.is_some(), cx)
            });
        }
        let now = Instant::now();
        if self
            .press
            .iter()
            .chain(&self.hover)
            .any(|motion| motion.moving(now, view.reduced))
            || self.icon.moving(now, view.reduced)
            || self.sources_loading && !view.reduced
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
                window.request_animation_frame();
            }))
            .child(page)
    }
}
