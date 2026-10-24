use crate::assets::Weighted;
use crate::{
    film_grid::{GridHost, NextCard, PreviousCard},
    hero,
    home_layout::{Grid, ViewSettings},
    image_cache::CachedImage,
    theme::{Theme, focus_ring, motion::SHIMMER},
    transition::bezier,
};
use gpui::{
    Context, Div, FocusHandle, FontWeight, Window, div, linear_color_stop, linear_gradient,
    prelude::*, px, relative,
};
use panorama_core::addons::FilmDetails;
use std::time::Instant;

/// Resources and interaction state for one fixed-size tile.
pub struct CardProps<'a> {
    pub(crate) film: Option<&'a FilmDetails>,
    pub(crate) image: Option<CachedImage>,
    pub(crate) poster: bool,
    pub(crate) unavailable: bool,
    pub(crate) focus: &'a FocusHandle,
    pub(crate) index: usize,
    pub(crate) shimmer: f32,
}

/// Moving surface highlight for unloaded artwork and skeleton cards. The band is two tiles
/// wide with its peak in the middle, so the peak travels from one tile-width before the left
/// edge to one past the right: each cycle starts and ends with the band wholly off the tile,
/// and the loop has no seam.
pub fn shimmer(view: ViewSettings, phase: f32) -> Div {
    let phase = bezier(phase, SHIMMER.easing);
    div().absolute().inset_0().overflow_hidden().child(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(gpui::relative(2.0))
            .left(gpui::relative(-2.0 + 3.0 * phase))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(gpui::relative(0.5))
                    .bg(linear_gradient(
                        90.0,
                        linear_color_stop(view.theme.surface, 0.0),
                        linear_color_stop(view.theme.surface_strong, 1.0),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .w(gpui::relative(0.5))
                    .bg(linear_gradient(
                        90.0,
                        linear_color_stop(view.theme.surface_strong, 0.0),
                        linear_color_stop(view.theme.surface, 1.0),
                    )),
            ),
    )
}

pub fn fade_in(image: &CachedImage, reduced: bool) -> f32 {
    if reduced {
        return 1.0;
    }
    bezier(
        (Instant::now().duration_since(image.loaded).as_secs_f32()
            / crate::theme::DURATION_STANDARD.as_secs_f32())
        .min(1.0),
        crate::theme::EASE_EDITORIAL,
    )
}

/// Render image, fades, credits and a full-tile focusable details button.
pub fn render<V: GridHost>(
    props: CardProps<'_>,
    grid: Grid,
    view: ViewSettings,
    window: &Window,
    cx: &mut Context<V>,
) -> Div {
    let theme = view.theme;
    let height = grid.height();
    let index = props.index;
    let mut tile = div()
        .relative()
        .w(px(grid.width))
        .h(px(height))
        .flex_shrink_0()
        .overflow_hidden()
        .bg(if props.unavailable {
            theme.artwork_empty
        } else {
            theme.surface
        });
    if !props.unavailable && props.image.is_none() {
        tile = tile.child(shimmer(view, props.shimmer));
    }
    if let Some(image) = props.image {
        let fade = fade_in(&image, view.reduced);
        tile = tile
            .child(hero::cover(image.image, (grid.width, height), props.poster, 1.0).opacity(fade));
    }
    let fade = theme
        .scrim
        .opacity(if props.film.is_some() { 0.58 } else { 0.28 });
    tile = tile.child(
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
    );
    if let Some(film) = props.film {
        tile = tile.child(caption(film, theme));
        let target = film.clone();
        let button = focus_ring(
            div()
                .id(("film-card", index))
                .key_context("PanoramaCard")
                .on_action(cx.listener(move |host: &mut V, _: &NextCard, window, cx| {
                    host.focus_card(index, true, window, cx)
                }))
                .on_action(
                    cx.listener(move |host: &mut V, _: &PreviousCard, window, cx| {
                        host.focus_card(index, false, window, cx)
                    }),
                )
                .absolute()
                .inset_0()
                .cursor_pointer()
                .role(gpui::Role::Button)
                .aria_label(format!("Open details for {}", film.name))
                .on_hover(cx.listener(move |host: &mut V, over, _, cx| {
                    host.film_grid().hover(index, *over);
                    cx.notify();
                }))
                .on_click(cx.listener(move |host: &mut V, _, window, cx| {
                    host.open_film(target.clone(), window, cx)
                })),
            props.focus,
            theme,
            view.active,
            view.keyboard,
            0.0,
            window,
        )
        .absolute();
        tile = tile
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_1()
                    .border_color(theme.image_outline),
            )
            .child(button);
    } else {
        tile = tile.child(
            div()
                .absolute()
                .left(px(18.0))
                .right(px(18.0))
                .bottom(px(14.0))
                .child(div().h(px(11.0)).w(relative(0.58)).bg(theme.glass))
                .child(
                    div()
                        .mt(px(8.0))
                        .h(px(11.0))
                        .w(relative(0.28))
                        .bg(theme.glass),
                ),
        );
    }
    tile
}

/// Title and credits pinned to the bottom of a tile, at fixed sizes like MUBI's cards.
pub fn caption(film: &FilmDetails, theme: Theme) -> Div {
    let credits = div()
        .flex()
        .min_w_0()
        .gap(px(4.0))
        .text_size(px(12.0))
        .line_height(relative(1.25))
        .when(!film.director.is_empty(), |row| {
            row.child(
                div()
                    .min_w_0()
                    .truncate()
                    .weight(FontWeight::MEDIUM)
                    .child(film.director.join(", ").to_uppercase()),
            )
        })
        .when_some(film.origin_country.clone(), |row, country| {
            row.child(
                div()
                    .flex_shrink_0()
                    .text_color(theme.ink)
                    .font_features(gpui::FontFeatures(std::sync::Arc::new(vec![
                        ("tnum".into(), 1),
                        ("kern".into(), 1),
                    ])))
                    .child(country.to_uppercase()),
            )
        })
        .when_some(film.release_info.clone(), |row, year| {
            row.child(
                div()
                    .flex_shrink_0()
                    .text_color(theme.ink)
                    .font_features(gpui::FontFeatures(std::sync::Arc::new(vec![
                        ("tnum".into(), 1),
                        ("kern".into(), 1),
                    ])))
                    .child(year),
            )
        });
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .w_full()
        .px(px(20.0))
        .pb(px(16.0))
        .pt(px(28.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .text_color(theme.ink)
        .child(
            div()
                .text_size(px(22.0))
                .weight(FontWeight::MEDIUM)
                .line_height(relative(1.05))
                .line_clamp(2)
                .child(film.name.to_uppercase()),
        )
        .child(credits)
}
