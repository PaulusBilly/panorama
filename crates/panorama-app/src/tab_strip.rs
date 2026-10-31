//! A row of text tabs: festival editions, a show's seasons.
use crate::{
    home_layout::ViewSettings,
    theme::{Typography, focus_ring},
};
use gpui::{Div, ElementId, FocusHandle, Stateful, Window, div, prelude::*, px};

const ROW: f32 = 34.0;
/// Each tab's side padding; the row hangs out by this so the first label meets the column.
const PAD: f32 = 10.0;

pub fn tab(
    id: impl Into<ElementId>,
    label: &str,
    selected: bool,
    focus: &FocusHandle,
    view: ViewSettings,
    window: &Window,
) -> Stateful<Div> {
    let theme = view.theme;
    focus_ring(
        div()
            .id(id)
            .role(gpui::Role::Tab)
            .aria_label(label.to_owned())
            .cursor_pointer()
            .flex_none()
            .px(px(PAD))
            .pt(px(8.0))
            .pb(px(6.0))
            .border_b_2()
            .border_color(if selected {
                theme.ink
            } else {
                gpui::transparent_black().into()
            })
            .label()
            .text_color(if selected { theme.ink } else { theme.ink_muted })
            .hover(|style| style.text_color(theme.ink))
            .child(label.to_owned()),
        focus,
        theme,
        view.active,
        view.keyboard,
        0.0,
        window,
    )
}

pub fn strip(
    id: &'static str,
    label: &'static str,
    width: f32,
    tabs: impl IntoIterator<Item = Stateful<Div>>,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(gpui::Role::TabList)
        .aria_label(label)
        .ml(px(-PAD))
        .max_w(px(width + PAD))
        // A scroll container takes no height from its row; this is one tab.
        .h(px(ROW))
        .overflow_x_scroll()
        .flex()
        .children(tabs)
}
