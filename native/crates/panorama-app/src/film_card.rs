use crate::{
    hero,
    home_layout::{Grid, ViewSettings},
    image_cache::CachedImage,
    routes::home::Home,
    theme::{focus_ring, motion::SHIMMER},
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
    pub(crate) hover: f32,
    pub(crate) index: usize,
    pub(crate) shimmer: f32,
}

/// Moving CSS surface gradient for unloaded artwork and skeleton cards.
pub fn shimmer(view: ViewSettings, phase: f32) -> Div {
    let phase = bezier(phase, SHIMMER.easing);
    div().absolute().inset_0().overflow_hidden().child(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(gpui::relative(2.0))
            .left(gpui::relative(-1.0 + 2.0 * phase))
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

/// Render image, fades, credits and a full-tile focusable details button.
pub fn render(
    props: CardProps<'_>,
    grid: Grid,
    view: ViewSettings,
    window: &Window,
    cx: &mut Context<Home>,
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
        let fade = if view.reduced {
            1.0
        } else {
            bezier(
                (Instant::now().duration_since(image.loaded).as_secs_f32()
                    / crate::theme::DURATION_STANDARD.as_secs_f32())
                .min(1.0),
                crate::theme::EASE_EDITORIAL,
            )
        };
        tile = tile.child(
            hero::cover(
                image.image,
                (grid.width, height),
                props.poster,
                1.0 + 0.012 * props.hover,
            )
            .opacity(fade),
        );
    }
    let fade = if props.film.is_some() {
        theme.artwork_fade
    } else {
        theme.skeleton_fade
    };
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
        let credits = div()
            .flex()
            .min_w_0()
            .gap(px(4.0))
            .text_size(px((view.width * 0.009).clamp(9.0, 11.0)))
            .line_height(relative(1.25))
            .when(!film.director.is_empty(), |row| {
                row.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::BOLD)
                        .child(film.director.join(", ").to_uppercase()),
                )
            })
            .when_some(film.origin_country.clone(), |row, country| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_color(theme.inverse.opacity(0.9))
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
                        .text_color(theme.inverse.opacity(0.9))
                        .font_features(gpui::FontFeatures(std::sync::Arc::new(vec![
                            ("tnum".into(), 1),
                            ("kern".into(), 1),
                        ])))
                        .child(year),
                )
            });
        tile = tile.child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .w_full()
                .px(px(18.0))
                .pb(px(14.0))
                .pt(px(28.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .text_color(theme.inverse)
                .child(
                    div()
                        .text_size(px((view.width * 0.018).clamp(16.0, 24.0)))
                        .font_weight(FontWeight::MEDIUM)
                        .line_height(relative(1.18))
                        .line_clamp(2)
                        .child(film.name.to_uppercase()),
                )
                .child(credits),
        );
        let id = film.id.clone();
        let button = focus_ring(
            div()
                .id(("film-card", index))
                .key_context("PanoramaCard")
                .on_action(cx.listener(
                    move |home, _: &crate::routes::home::NextCard, window, cx| {
                        home.focus_card(index, true, window, cx)
                    },
                ))
                .on_action(cx.listener(
                    move |home, _: &crate::routes::home::PreviousCard, window, cx| {
                        home.focus_card(index, false, window, cx)
                    },
                ))
                .absolute()
                .inset_0()
                .cursor_pointer()
                .role(gpui::Role::Button)
                .aria_label(format!("Open details for {}", film.name))
                .on_hover(cx.listener(move |home, over, _, cx| home.hover(index, *over, cx)))
                .on_click(
                    cx.listener(move |home, _, window, cx| home.open_film(id.clone(), window, cx)),
                ),
            props.focus,
            theme,
            view.active,
            view.keyboard,
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
                .child(
                    div()
                        .h(px(11.0))
                        .w(relative(0.58))
                        .bg(theme.inverse.opacity(0.4)),
                )
                .child(
                    div()
                        .mt(px(8.0))
                        .h(px(11.0))
                        .w(relative(0.28))
                        .bg(theme.inverse.opacity(0.4)),
                ),
        );
    }
    tile
}
