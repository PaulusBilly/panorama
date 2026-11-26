//! A row of text tabs, as MUBI sets a show's seasons: festival editions, a show's seasons.
use crate::{
    home_layout::ViewSettings,
    theme::{Theme, caps, focus_ring, typography::SECTION},
};
use gpui::{Div, ElementId, FocusHandle, Stateful, Window, div, prelude::*, px};

/// One tab: its label, the space under it, and its bar.
const ROW: f32 = 44.0;
/// Between labels.
const GAP: f32 = 36.0;
/// The selected tab's bar, which sits on the strip's hairline.
const BAR: f32 = 3.0;

/// A tab, its label set in capitals like a section heading.
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
            .h(px(ROW))
            .flex()
            .flex_col()
            .justify_end()
            .text_color(if selected { theme.ink } else { theme.ink_muted })
            .hover(|style| style.text_color(theme.ink))
            .child(
                SECTION
                    .apply(div())
                    .text_size(px(SECTION.size + 2.0))
                    .pb(px(12.0))
                    .child(caps(label)),
            )
            .child(div().h(px(BAR)).when(selected, |bar| bar.bg(theme.ink))),
        focus,
        theme,
        view.active,
        view.keyboard,
        0.0,
        window,
    )
}

/// The tabs in a row across `width`, over a hairline the selected tab's bar rests on.
pub fn strip(
    id: &'static str,
    label: &'static str,
    width: f32,
    theme: Theme,
    tabs: impl IntoIterator<Item = Stateful<Div>>,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(gpui::Role::TabList)
        .aria_label(label)
        .w(px(width))
        // A scroll container takes no height from its row; this is one tab.
        .h(px(ROW + 1.0))
        .overflow_x_scroll()
        .border_b_1()
        .border_color(theme.rule)
        .flex()
        .gap(px(GAP))
        .children(tabs)
}
