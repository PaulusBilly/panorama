use super::Theme;
use gpui::{Div, FocusHandle, Stateful, Window, div, prelude::*, px};

/// Apply the inset outline and outer keyboard focus ring to a control.
///
/// The outline follows the control's own corner radius: 0 for cards and rows, h/2 for pills.
pub fn focus_ring(
    element: Stateful<Div>,
    focus: &FocusHandle,
    theme: Theme,
    active: bool,
    keyboard_navigation: bool,
    radius: f32,
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
                        .rounded(px(radius + 2.0))
                        .border_2()
                        .border_color(theme.focus_outer),
                )
                .child(
                    div()
                        .absolute()
                        .inset(px(2.0))
                        .rounded(px((radius - 2.0).max(0.0)))
                        .border_2()
                        .border_color(theme.focus_inner),
                )
        })
}
