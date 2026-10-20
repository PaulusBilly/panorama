//! Small elements shared by the Addons sections.
use crate::{
    hero,
    image_cache::CachedImage,
    theme::{Theme, Typography, focus_ring},
};
use gpui::{
    Div, ElementId, FocusHandle, Rgba, SharedString, Stateful, Svg, Transformation, Window, div,
    prelude::*, px, relative, svg,
};

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

/// An underlined 13 px text button, the app's quiet action.
pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    leading: Option<Svg>,
    color: Rgba,
    focus: &FocusHandle,
    view: View,
    window: &Window,
) -> Stateful<Div> {
    let label = label.into();
    focus_ring(
        div()
            .id(id)
            .min_h(px(40.0))
            .px(px(4.0))
            .flex_shrink_0()
            .label()
            .text_color(color)
            .border_b_1()
            .border_color(color)
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_pointer()
            .role(gpui::Role::Button)
            .aria_label(label.clone())
            .children(leading)
            .child(label),
        focus,
        view.theme,
        view.active,
        view.keyboard,
        0.0,
        window,
    )
}

/// A 36 px square icon button with an 18 px glyph.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: impl Into<SharedString>,
    enabled: bool,
    focus: &FocusHandle,
    view: View,
    window: &Window,
) -> Stateful<Div> {
    let theme = view.theme;
    focus_ring(
        div()
            .id(id)
            .size(px(36.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .role(gpui::Role::Button)
            .aria_label(label)
            .when(enabled, |button| {
                button.cursor_pointer().hover(|s| s.bg(theme.hover))
            })
            .child(svg().path(icon).size(px(18.0)).text_color(if enabled {
                theme.ink
            } else {
                theme.disabled
            })),
        focus,
        theme,
        view.active && enabled,
        view.keyboard,
        0.0,
        window,
    )
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

/// A square logo, or the name's first letter on `surface`.
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
        .bg(theme.surface);
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
