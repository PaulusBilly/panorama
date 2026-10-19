//! The message row with one underlined action, shared by Home and Addons.
use crate::theme::{Theme, Typography, focus_ring};
use gpui::{App, ClickEvent, Div, FocusHandle, Window, div, prelude::*, px, relative};

/// A hairline-bounded message with a single text button.
pub struct AlertRow<'a> {
    /// Plain-text message.
    pub message: String,
    /// Button label, also its accessible name and element id.
    pub label: &'static str,
    /// Active palette.
    pub theme: Theme,
    /// Viewport width, for the narrow stacked layout.
    pub width: f32,
    /// Focus target of the button.
    pub focus: &'a FocusHandle,
    /// Whether the owning route is current.
    pub active: bool,
    /// Whether to draw keyboard focus.
    pub keyboard: bool,
}

impl AlertRow<'_> {
    /// Render the row; `on_click` runs when the button is activated.
    pub fn render(
        self,
        window: &Window,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Div {
        let theme = self.theme;
        div()
            .mt(px(32.0))
            .py(px(22.0))
            .border_y_1()
            .border_color(theme.rule)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(32.0))
            .when(self.width < 700.0, |row| row.flex_col().items_start())
            .child(div().body().child(self.message))
            .child(focus_ring(
                div()
                    .id(self.label)
                    .min_h(px(40.0))
                    .px(px(4.0))
                    .text_size(px(13.0))
                    .line_height(relative(1.4))
                    .border_b_1()
                    .border_color(theme.ink)
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .role(gpui::Role::Button)
                    .aria_label(self.label)
                    .on_click(on_click)
                    .child(self.label),
                self.focus,
                theme,
                self.active,
                self.keyboard,
                window,
            ))
    }
}
