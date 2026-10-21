//! Small elements shared by the Addons sections.
use crate::{
    hero,
    image_cache::CachedImage,
    theme::{Theme, Typography},
};
use gpui::{Div, Rgba, Svg, Transformation, div, prelude::*, px, relative, svg};

/// Per-frame settings every widget reads.
#[derive(Clone, Copy)]
pub struct View {
    pub theme: Theme,
    pub active: bool,
    pub keyboard: bool,
    pub reduced: bool,
    pub width: f32,
    /// Spinner rotation in radians.
    pub spin: f32,
}

/// Tabular figures, as the rest of the app sets versions and counts.
pub fn tnum() -> gpui::FontFeatures {
    gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]))
}

/// The rotating Tabler `loader-2`.
pub fn spinner(size: f32, color: Rgba, view: View) -> Svg {
    svg()
        .path("loader-2.svg")
        .size(px(size))
        .flex_shrink_0()
        .text_color(color)
        .when(!view.reduced, |icon| {
            icon.with_transformation(Transformation::rotate(gpui::radians(view.spin)))
        })
}

/// A hairline-topped section with its uppercase heading row.
pub fn section(view: View, heading: &str, trailing: Option<String>) -> Div {
    let theme = view.theme;
    div()
        .mt(px(56.0))
        .border_t_1()
        .border_color(theme.rule)
        .child(
            div()
                .mt(px(24.0))
                .flex()
                .items_baseline()
                .justify_between()
                .child(
                    div()
                        .label()
                        .text_color(theme.ink)
                        .child(heading.to_uppercase()),
                )
                .when_some(trailing, |row, text| {
                    row.child(div().caption().text_color(theme.ink_muted).child(text))
                }),
        )
}

/// The `surface` panel a section's rows sit in; rows divide themselves with `rule`.
pub fn panel(theme: Theme) -> Div {
    div().mt(px(16.0)).px(px(20.0)).bg(theme.surface)
}

/// A square logo, or the name's first letter on `surface_strong`.
pub fn logo(size: f32, name: &str, image: Option<CachedImage>, large: bool, theme: Theme) -> Div {
    let letter = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default();
    let tile = div()
        .relative()
        .size(px(size))
        .flex_shrink_0()
        .overflow_hidden()
        .bg(theme.surface_strong);
    let tile = match image {
        Some(image) => tile.child(hero::cover(image.image, (size, size), false, 1.0)),
        None => tile
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.ink_muted)
            .when(large, |tile| tile.title())
            .when(!large, |tile| tile.heading())
            .line_height(relative(1.0))
            .child(letter),
    };
    tile.child(
        div()
            .absolute()
            .inset_0()
            .border_1()
            .border_color(theme.image_outline),
    )
}
