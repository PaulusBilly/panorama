use super::*;
use crate::assets::Weighted;
use crate::{
    film_card,
    film_display::{self, Quality},
    hero,
    theme::{
        DURATION_STANDARD, EASE_EDITORIAL, Typography, caps,
        scrim::{Edge, scrim},
        typography::{BODY, CREDIT_PLACE, LABEL},
    },
    titlebar::SAFE_TOP,
    transition::bezier,
};
use gpui::{Div, FontWeight, Render, div, img, point, prelude::*, px, relative, svg};

/// Space above and below the genre line, so it sits centred between origin and quality.
const GENRE_GAP: f32 = 12.0;
/// Windows wider than this split the synopsis into its own column, as MUBI does.
const SPLIT_MIN: f32 = 960.0;
const SPLIT_GAP: f32 = 64.0;
use crate::hero::FILM_LOGO;
use std::time::Duration;
/// Width of the left column when the synopsis is split out.
fn info_width(view: ViewSettings) -> f32 {
    (view.column().width * 0.3).clamp(380.0, 460.0)
}
const SYNOPSIS_WIDTH: f32 = 620.0;
/// With nothing playable yet, sources can take 15 s; the page stops holding for them here.
const REVEAL_LIMIT: Duration = Duration::from_secs(4);
/// A text title wraps here. On wide windows it has its own row, so it never moves the synopsis.
const TITLE_WIDTH: f32 = 560.0;
/// The hero stays sharp until this share of the window has scrolled by.
const BLUR_START: f32 = 0.12;
/// Fully blurred by here, just before the sections below the hero reach mid-window.
const BLUR_END: f32 = 0.45;
/// How far the blurred still dims under the sections' text.
const BLUR_WASH: f32 = 0.6;

fn blur_progress(top: f32, height: f32) -> f32 {
    ((top / height.max(1.0) - BLUR_START) / (BLUR_END - BLUR_START)).clamp(0.0, 1.0)
}
/// Quality badge height; the clock's visible disc matches it.
const BADGE: f32 = 18.0;
/// The clock glyph's disc is 20 of the icon's 24 units, so the icon is drawn this much larger.
const CLOCK_INSET: f32 = 24.0 / 20.0;

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
            .map(|url| self.image(url, FILM_LOGO, view, window, cx));
        let mut title = div().max_w(px(TITLE_WIDTH));
        if let Some((Some(logo), false)) = logo.as_ref() {
            let size = logo.image.size(0);
            let ratio = (FILM_LOGO.0 / size.width.0 as f32)
                .min(FILM_LOGO.1 / size.height.0 as f32)
                .min(1.0 / window.scale_factor());
            title = title.child(
                img(logo.image.clone())
                    .w(px(size.width.0 as f32 * ratio))
                    .h(px(size.height.0 as f32 * ratio)),
            );
        } else if logo.is_none() || logo.as_ref().is_some_and(|(_, failed)| *failed) {
            title = title.child(
                hero::title_style(view.width)
                    .apply(div())
                    .line_clamp(3)
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
        // Wide windows give the synopsis its own column from the middle of the page, bottom
        // aligned with the actions, under the title's row; narrow ones keep it all in one column.
        let split = view.width > SPLIT_MIN;
        let (above, inline) = if split {
            (Some(title), None)
        } else {
            (None, Some(title))
        };
        let mut column = div()
            .w_full()
            .max_w(px(640.0))
            .flex()
            .flex_col()
            .text_color(theme.ink)
            .children(inline)
            .child(hero::byline(meta))
            .when(!meta.genres.is_empty(), |column| {
                column.child(
                    CREDIT_PLACE
                        .apply(div())
                        .mt(px(GENRE_GAP))
                        .text_color(theme.ink_muted)
                        .child(meta.genres.join(", ")),
                )
            });
        let mut facts = LABEL
            .apply(div())
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(8.0))
            .gap_y(px(8.0))
            .text_color(theme.ink_muted)
            .font_features(gpui::FontFeatures(Arc::new(vec![
                ("tnum".into(), 1),
                ("kern".into(), 1),
            ])));
        let mut any = false;
        for badge in quality
            .video
            .into_iter()
            .chain(quality.surround.then_some("5.1"))
        {
            any = true;
            facts = facts.child(
                div()
                    .h(px(BADGE))
                    .flex()
                    .items_center()
                    .border_1()
                    .border_color(theme.ink_muted)
                    .px(px(6.0))
                    .text_size(px(11.0))
                    .weight(FontWeight::MEDIUM)
                    .line_height(relative(1.0))
                    .child(badge),
            );
        }
        if let Some(runtime) = &meta.runtime {
            any = true;
            facts = facts.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(
                        svg()
                            .path("clock-filled.svg")
                            .size(px(BADGE * CLOCK_INSET))
                            .text_color(theme.ink_muted),
                    )
                    .child(film_display::runtime(runtime)),
            );
        }
        // The row holds its height for quality badges, so late sources never move the title.
        if any || self.sources_loading {
            column = column.child(facts.min_h(px(BADGE)).mt(px(GENRE_GAP)));
        }
        let synopsis = BODY
            .apply(div())
            .text_size(px(15.0))
            .text_color(theme.ink)
            .child(
                meta.description
                    .clone()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| "No synopsis is available for this film.".into()),
            );
        if !split {
            column = column.child(synopsis.mt(px(20.0)));
            return self.finish_column(column, view, cx);
        }
        let column = self.finish_column(column, view, cx);
        div().w_full().flex().flex_col().children(above).child(
            div()
                .w_full()
                .flex()
                .items_end()
                .gap(px(SPLIT_GAP))
                .child(column.flex_none().w(px(info_width(view))))
                .child(synopsis.flex_1().max_w(px(SYNOPSIS_WIDTH))),
        )
    }
    fn finish_column(
        &mut self,
        mut column: Div,
        view: ViewSettings,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        column = column.child(self.actions(view, cx).mt(px(28.0)));
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
        // Debug scrolls wait for the reveal: before it there is nothing below the hero.
        if view.active
            && self.revealed.is_some()
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
        let theme = view.theme;
        let col = view.column();
        if view.active {
            self.images.update(cx, |images, _| images.begin_frame());
        }
        // The card's preview artwork differs from the details', so wait for the details.
        let (artwork, poster, source) = if self.loading && self.revealed.is_none() {
            (None, false, None)
        } else {
            self.artwork(view, window, cx)
        };
        let blurred = source
            .as_ref()
            .filter(|_| view.active)
            .and_then(|url| self.blurred(url, window, cx));
        if self.revealed.is_none() {
            let logo = self.metadata.as_ref().and_then(|meta| meta.logo.clone());
            let logo_pending = logo.is_some_and(|url| {
                matches!(self.image(&url, FILM_LOGO, view, window, cx), (None, false))
            });
            let art_pending = source.is_some() && artwork.is_none();
            let waiting =
                art_pending || logo_pending || self.sources_loading || self.original_pending;
            if view.active && !self.loading && (!waiting || self.launched.elapsed() >= REVEAL_LIMIT)
            {
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
            .text_color(theme.ink);
        let blur = blur_progress(-f32::from(self.scroll.offset().y), view.height);
        // The page scrolls over this. The card's color holds until the still fades in; past the
        // threshold the blurred copy takes over under an even wash. The bottom scrim stays.
        let bounds = (view.width, view.height);
        let backdrop = div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .bg(theme.canvas)
            .when_some(self.tint, |backdrop, tint| {
                backdrop.child(div().absolute().inset_0().bg(tint))
            })
            .when_some(artwork.filter(|_| reveal > 0.0), |backdrop, art| {
                backdrop.child(
                    div()
                        .absolute()
                        .inset_0()
                        .opacity(reveal * film_card::fade_in(&art, view.reduced))
                        .bg(theme.artwork_empty)
                        .child(hero::cover(art.image, bounds, poster, 1.0)),
                )
            })
            .when_some(blurred.filter(|_| blur > 0.0), |backdrop, art| {
                backdrop.child(hero::cover(art.image, bounds, poster, 1.0).opacity(blur))
            })
            .child(div().absolute().inset_0().opacity(1.0 - blur).child(scrim(
                theme,
                Edge::Top,
                relative(0.3),
                0.6,
            )))
            .child(scrim(theme, Edge::Bottom, relative(0.45), 1.0))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(theme.canvas.opacity(BLUR_WASH * blur)),
            );
        let metadata = self.metadata.clone();
        let mut content = div()
            .relative()
            .w(px(col.width))
            .ml(px(col.left))
            .min_h(px(view.height))
            .pt(px(SAFE_TOP + 72.0))
            // The actions sit level with the avatar, as the back button sits level with the logo.
            .pb(px(crate::sidebar::AVATAR_INSET))
            .flex()
            .flex_col()
            .justify_end()
            .child(
                div()
                    .absolute()
                    .top(px(
                        crate::sidebar::LOGO_CENTER - crate::button::Size::Regular.height() * 0.5
                    ))
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
                    .child(self.retry(view, cx)),
            );
        } else if let Some(meta) = metadata.as_ref().filter(|_| self.revealed.is_some()) {
            content = content.child(
                self.metadata_content(meta, view, window, cx)
                    .opacity(reveal),
            );
        } else {
            content = content.child(
                div()
                    .id("film-loading")
                    .role(gpui::Role::Status)
                    .aria_label("Loading film metadata"),
            );
        }
        page = page.child(content);
        if self.revealed.is_some()
            && !self.details_error
            && let Some(sections) = self.credits_sections(view, window, cx)
        {
            page = page.child(sections.opacity(reveal));
        }
        if view.active {
            self.images.update(cx, |images, _| images.finish_frame());
        }
        let holding = self.revealed.is_none() && self.launched.elapsed() < REVEAL_LIMIT;
        if holding
            || reveal < 1.0 && !view.reduced
            || view.active && self.images.read(cx).moving()
            || self.people_rail.moving(view.reduced)
            || self.awards_rail.moving(view.reduced)
            || self.debug_step < self.args.scroll.len()
        {
            cx.on_next_frame(window, |film, window, cx| film.tick(window, cx));
            window.request_animation_frame();
        }
        let scroller = div()
            .id("film-scroll")
            .size_full()
            .overflow_y_scroll()
            // Sideways swipes belong to the carousels; without this the page turns them
            // into vertical scrolling.
            .restrict_scroll_to_axis()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|_, _, window, cx| {
                cx.on_next_frame(window, |film, window, cx| film.tick(window, cx));
                // request_animation_frame panics outside render; notify schedules the frame instead.
                cx.notify();
            }))
            .child(page);
        div().relative().size_full().child(backdrop).child(scroller)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_holds_off_until_the_threshold_then_finishes() {
        assert_eq!(blur_progress(0.0, 1000.0), 0.0);
        assert_eq!(blur_progress(120.0, 1000.0), 0.0);
        assert!((blur_progress(285.0, 1000.0) - 0.5).abs() < 0.001);
        assert_eq!(blur_progress(450.0, 1000.0), 1.0);
        assert_eq!(blur_progress(5000.0, 1000.0), 1.0);
    }
}
