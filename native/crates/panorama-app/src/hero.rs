use crate::{
    app::AppShell,
    home_layout::ViewSettings,
    image_cache::CachedImage,
    router::Route,
    theme::{Theme, content_width, focus_ring},
};
use gpui::{
    Div, FocusHandle, FontWeight, RenderImage, WeakEntity, Window, div, img, prelude::*, px,
    relative, svg,
};
use panorama_core::{addons::FilmDetails, images::DecodedImage};
use std::sync::Arc;

/// Render a single size-specific CSS radial-over-linear overlay off the UI thread.
pub fn overlay(width: u32, height: u32, theme: Theme) -> DecodedImage {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    let center = width as f32 * 0.18;
    let radius = ((width as f32 - center).powi(2) + (height as f32).powi(2)).sqrt() * 0.58;
    for y in 0..height {
        let t = (y as f32 + 0.5) / height as f32;
        let (from, to, u) = if t <= 0.44 {
            (theme.hero_top, theme.hero_mid, t / 0.44)
        } else {
            (theme.hero_mid, theme.hero_bottom, (t - 0.44) / 0.56)
        };
        let va = from.a + (to.a - from.a) * u;
        let rgb = |f: f32, t: f32| {
            if va > 0.0 {
                (f * from.a * (1.0 - u) + t * to.a * u) / va
            } else {
                0.0
            }
        };
        let vertical = [rgb(from.r, to.r), rgb(from.g, to.g), rgb(from.b, to.b)];
        for x in 0..width {
            let distance = ((x as f32 + 0.5 - center).powi(2)
                + (y as f32 + 0.5 - height as f32).powi(2))
            .sqrt();
            let ra = theme.hero_copy_fade.a * (1.0 - distance / radius).clamp(0.0, 1.0);
            let alpha = ra + va * (1.0 - ra);
            let copy = theme.hero_copy_fade;
            for (radial, vertical) in [copy.r, copy.g, copy.b].into_iter().zip(vertical) {
                let channel = if alpha > 0.0 {
                    (radial * ra + vertical * va * (1.0 - ra)) / alpha
                } else {
                    0.0
                };
                rgba.push((channel * 255.0).round() as u8);
            }
            rgba.push((alpha * 255.0).round() as u8);
        }
    }
    DecodedImage {
        width,
        height,
        rgba,
    }
}

/// Featured film resources and sampled overlay opacity.
pub struct HeroProps<'a> {
    pub(crate) film: Option<&'a FilmDetails>,
    pub(crate) artwork: Option<CachedImage>,
    pub(crate) poster: bool,
    pub(crate) logo: Option<CachedImage>,
    pub(crate) title_logo: bool,
    pub(crate) overlay: Option<Arc<RenderImage>>,
    pub(crate) surface: f32,
    pub(crate) focus: &'a FocusHandle,
    pub(crate) shell: WeakEntity<AppShell>,
    pub(crate) hover: f32,
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
        .text_color(theme.inverse)
        .aria_label("Featured film");
    if let Some(art) = props.artwork {
        hero = hero.child(cover(
            art.image,
            (view.width, view.height),
            props.poster,
            1.0,
        ));
    }
    if let Some(overlay) = props.overlay {
        hero = hero.child(img(overlay).absolute().inset_0().size_full());
    }
    if let Some(film) = props.film {
        let measure = |text: &str, size: f32, weight: FontWeight| {
            let text = text.replace(['\r', '\n'], " ");
            let mut font = window.text_style().font();
            font.weight = weight;
            let run = gpui::TextRun {
                len: text.len(),
                font,
                ..Default::default()
            };
            f32::from(
                window
                    .text_system()
                    .shape_line(text.into(), px(size), &[run], None)
                    .width,
            )
        };
        let credit_width = measure("DIRECTED BY ", 14.0, FontWeight::NORMAL)
            + measure(
                &film.director.join(", ").to_uppercase(),
                14.0,
                FontWeight::BOLD,
            );
        let country_year = [film.origin_country.as_deref(), film.release_info.as_deref()]
            .into_iter()
            .flatten()
            .map(str::to_uppercase)
            .collect::<Vec<_>>()
            .join("  ");
        let logo_bounds = props.logo.as_ref().map(|logo| {
            let size = logo.image.size(0);
            let width = size.width.0 as f32 / window.scale_factor();
            let height = size.height.0 as f32 / window.scale_factor();
            let ratio = (310.0 / width).min(128.0 / height).min(1.0);
            (width * ratio, height * ratio)
        });
        let title_width = if props.title_logo {
            logo_bounds.map_or(0.0, |bounds| bounds.0).max(credit_width)
        } else if !film.director.is_empty() {
            credit_width.max(measure(&country_year, 14.0, FontWeight::NORMAL))
        } else {
            measure(&film.name.to_uppercase(), 32.0, FontWeight::MEDIUM)
        };
        let title_width = title_width.min(content_width(view.width));
        let mut title = div().flex().flex_col().w(px(title_width));
        if let Some(logo) = props.logo {
            let bounds = logo_bounds.unwrap_or((0.0, 0.0));
            title = title.child(img(logo.image).w(px(bounds.0)).h(px(bounds.1)));
        } else if !props.title_logo {
            title = title.child(
                div()
                    .text_size(px(32.0))
                    .font_weight(FontWeight::MEDIUM)
                    .line_height(relative(0.9))
                    .child(film.name.to_uppercase()),
            );
        }
        if !film.director.is_empty() || film.origin_country.is_some() || film.release_info.is_some()
        {
            let credits = credits(film, theme).mt(px(16.0));
            title = title.child(credits);
        }
        let shell = props.shell;
        let id = film.id.clone();
        let foreground = mix(theme.ink, theme.inverse, props.hover);
        let watch = focus_ring(
            div()
                .id("hero-watch")
                .key_context("PanoramaCard")
                .on_action(
                    cx.listener(|home, _: &crate::routes::home::NextCard, window, cx| {
                        home.focus_first(window, cx)
                    }),
                )
                .on_action(
                    cx.listener(|_, _: &crate::routes::home::PreviousCard, window, cx| {
                        window.focus_prev(cx);
                        cx.stop_propagation();
                    }),
                )
                .w(px(measure("WATCH", 16.0, FontWeight::MEDIUM) + 58.0))
                .min_h(px(44.0))
                .px(px(16.0))
                .border_1()
                .border_color(theme.inverse)
                .bg(theme.inverse.opacity(1.0 - props.hover))
                .text_color(foreground)
                .text_size(px(16.0))
                .font_weight(FontWeight::MEDIUM)
                .flex()
                .items_center()
                .gap(px(8.0))
                .cursor_pointer()
                .role(gpui::Role::Button)
                .aria_label(format!("WATCH {}", film.name))
                .on_hover(cx.listener(|home, over, _, cx| home.hover_watch(*over, cx)))
                .on_click(move |_, window, cx| {
                    let _ = shell.update(cx, |shell, cx| {
                        shell.navigate(Route::Player { id: id.clone() }, window, cx)
                    });
                })
                .child(
                    svg()
                        .path("player-play-filled.svg")
                        .size(px(16.0))
                        .text_color(foreground),
                )
                .child("WATCH")
                .group("hero-watch"),
            props.focus,
            theme,
            view.active,
            view.keyboard,
            window,
        );
        let row = div()
            .mt(px(if view.width < 900.0 { 44.0 } else { 24.0 }))
            .flex()
            .items_center()
            .gap(px(if view.width < 900.0 { 24.0 } else { 70.0 }))
            .when(view.width < 900.0, |row| row.flex_col().items_start())
            .child(
                div()
                    .w(px(if view.width < 900.0 { 0.0 } else { title_width }))
                    .when(view.width < 900.0, |row| row.w_auto())
                    .child(watch),
            )
            .when_some(film.description.clone(), |row, description| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .max_w(px(650.0))
                        .text_size(px(14.0))
                        .line_height(relative(1.55))
                        .text_color(theme.inverse.opacity(0.85))
                        .child(description),
                )
            });
        hero = hero.child(
            div()
                .relative()
                .h_full()
                .w(px(content_width(view.width)))
                .mx_auto()
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

fn mix(a: gpui::Rgba, b: gpui::Rgba, t: f32) -> gpui::Rgba {
    gpui::Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// Shared director and country-year block used by Home and Film.
pub fn credits(film: &FilmDetails, theme: Theme) -> Div {
    let mut credits = div()
        .text_size(px(14.0))
        .line_height(relative(1.0))
        .text_color(theme.inverse.opacity(0.85))
        .flex()
        .flex_col();
    if !film.director.is_empty() {
        credits = credits.child(
            div().flex().child("DIRECTED BY ").child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(film.director.join(", ").to_uppercase()),
            ),
        );
    }
    credits = credits.child(
        div()
            .mt(px(2.0))
            .flex()
            .gap(px(8.0))
            .font_features(gpui::FontFeatures(Arc::new(vec![
                ("tnum".into(), 1),
                ("kern".into(), 1),
            ])))
            .when_some(film.origin_country.clone(), |row, country| {
                row.child(country.to_uppercase())
            })
            .when_some(film.release_info.clone(), |row, year| row.child(year)),
    );
    credits
}
