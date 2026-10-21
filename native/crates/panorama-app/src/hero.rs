use crate::assets::Weighted;
use crate::{
    app::AppShell,
    button::{Button, Icon},
    home_layout::ViewSettings,
    image_cache::CachedImage,
    router::Route,
    theme::{
        caps, credits,
        scrim::{Edge, scrim},
    },
};
use gpui::{
    Div, FocusHandle, FontWeight, RenderImage, WeakEntity, Window, div, img, prelude::*, px,
    relative,
};
use panorama_core::addons::FilmDetails;
use std::sync::Arc;

/// Featured film resources and sampled surface opacity.
pub struct HeroProps<'a> {
    pub(crate) film: Option<&'a FilmDetails>,
    pub(crate) artwork: Option<CachedImage>,
    pub(crate) poster: bool,
    pub(crate) logo: Option<CachedImage>,
    pub(crate) title_logo: bool,
    pub(crate) surface: f32,
    pub(crate) focus: &'a FocusHandle,
    pub(crate) shell: WeakEntity<AppShell>,
}

/// Draw the viewport-sized hero with bottom-aligned editorial copy.
pub fn render(
    props: HeroProps<'_>,
    view: ViewSettings,
    window: &Window,
    cx: &mut gpui::Context<crate::routes::home::Home>,
) -> gpui::Stateful<Div> {
    let theme = view.theme;
    let mut hero = div()
        .id("featured-film")
        .relative()
        .w_full()
        .h(px(view.height))
        .flex_shrink_0()
        .overflow_hidden()
        .bg(theme.artwork_empty)
        .text_color(theme.ink)
        .aria_label("Featured film");
    if let Some(art) = props.artwork {
        hero = hero.child(cover(
            art.image,
            (view.width, view.height),
            props.poster,
            1.0,
        ));
    }
    hero = hero
        .child(scrim(theme, Edge::Left, relative(0.65), 0.92))
        .child(scrim(theme, Edge::Bottom, relative(0.45), 1.0));
    if let Some(film) = props.film {
        let mut title = div().flex().flex_col().max_w(px(view.column().width));
        if let Some(logo) = props.logo {
            let size = logo.image.size(0);
            let width = size.width.0 as f32 / window.scale_factor();
            let height = size.height.0 as f32 / window.scale_factor();
            let ratio = (310.0 / width).min(128.0 / height).min(1.0);
            title = title.child(img(logo.image).w(px(width * ratio)).h(px(height * ratio)));
        } else if !props.title_logo {
            title = title.child(
                div()
                    .text_size(px(32.0))
                    .weight(FontWeight::MEDIUM)
                    .line_height(relative(0.9))
                    .child(caps(&film.name)),
            );
        }
        if !film.director.is_empty() || film.origin_country.is_some() || film.release_info.is_some()
        {
            title = title.child(credits(film, theme).mt(px(16.0)));
        }
        let shell = props.shell;
        let id = film.id.clone();
        let watch = div()
            .key_context("PanoramaCard")
            .on_action(
                cx.listener(|home, _: &crate::film_grid::NextCard, window, cx| {
                    home.focus_first(window, cx)
                }),
            )
            .on_action(
                cx.listener(|_, _: &crate::film_grid::PreviousCard, window, cx| {
                    window.focus_prev(cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                Button::primary("hero-watch", "Watch")
                    .leading(Icon::Play)
                    .aria_label(format!("Watch {}", film.name))
                    .focus(props.focus, view.keyboard)
                    .tab_stop(view.active)
                    .on_click(move |_, window, cx| {
                        let _ = shell.update(cx, |shell, cx| {
                            shell.navigate(Route::Player { id: id.clone() }, window, cx)
                        });
                    }),
            );
        let row = div()
            .mt(px(if view.width < 900.0 { 44.0 } else { 24.0 }))
            .flex()
            .items_center()
            .gap(px(if view.width < 900.0 { 24.0 } else { 48.0 }))
            .when(view.width < 900.0, |row| row.flex_col().items_start())
            .child(watch)
            .when_some(film.description.clone(), |row, description| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .max_w(px(650.0))
                        .text_size(px(14.0))
                        .line_height(relative(1.55))
                        .text_color(theme.ink)
                        .child(description),
                )
            });
        hero = hero.child(
            div()
                .relative()
                .h_full()
                .w(px(view.column().width))
                .ml(px(view.column().left))
                .pt(px((view.height * 0.12).clamp(112.0, 144.0)))
                .pb(px(40.0))
                .flex()
                .flex_col()
                .justify_end()
                .child(title)
                .child(row),
        );
    }
    hero.child(
        div()
            .absolute()
            .inset_0()
            .bg(theme.surface)
            .opacity(props.surface),
    )
}

/// CSS cover with explicit 50% 28% positioning for poster fallback.
pub fn cover(image: Arc<RenderImage>, bounds: (f32, f32), poster: bool, scale: f32) -> Div {
    let size = image.size(0);
    let ratio = (bounds.0 / size.width.0 as f32).max(bounds.1 / size.height.0 as f32) * scale;
    let width = size.width.0 as f32 * ratio;
    let height = size.height.0 as f32 * ratio;
    div().absolute().inset_0().overflow_hidden().child(
        img(image)
            .absolute()
            .w(px(width))
            .h(px(height))
            .left(px((bounds.0 - width) * 0.5))
            .top(px((bounds.1 - height) * if poster { 0.28 } else { 0.5 })),
    )
}
