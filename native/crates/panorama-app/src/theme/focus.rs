use super::Theme;
use gpui::{Div, FocusHandle, Stateful, Window, div, prelude::*, px};

/// Apply Electron's inset outline and outer keyboard focus ring to a control.
pub fn focus_ring(
    element: Stateful<Div>,
    focus: &FocusHandle,
    theme: Theme,
    active: bool,
    keyboard_navigation: bool,
    window: &Window,
) -> Stateful<Div> {
    let visible = active && keyboard_navigation && focus.is_focused(window);
    element
        .relative()
        .track_focus(&focus.clone().tab_stop(active))
        .when(visible, |element| {
            element
                .child(
                    div()
                        .absolute()
                        .inset(px(-2.0))
                        .border_2()
                        .border_color(theme.focus_outer),
                )
                .child(
                    div()
                        .absolute()
                        .inset(px(2.0))
                        .border_2()
                        .border_color(theme.focus_inner),
                )
        })
}
