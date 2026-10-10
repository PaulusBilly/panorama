//! Shared three-column Panorama header.
use crate::{
    app::AppShell,
    router::Route,
    theme::{Theme, Typography, content_width, focus_ring},
};
use gpui::{Context, Div, FocusHandle, Window, div, prelude::*, px, svg};

/// Retained keyboard focus for the shared header controls.
pub struct Header {
    back: FocusHandle,
    home: FocusHandle,
    account: FocusHandle,
}

impl Header {
    /// Allocate the shared focus handles.
    pub fn new(cx: &mut Context<AppShell>) -> Self {
        Self {
            back: cx.focus_handle(),
            home: cx.focus_handle(),
            account: cx.focus_handle(),
        }
    }

    /// Render the leading, centered logo, and trailing slots.
    pub fn render(
        &self,
        route: &Route,
        active: bool,
        theme: Theme,
        keyboard_navigation: bool,
        window: &Window,
        cx: &mut Context<AppShell>,
    ) -> Div {
        let width: f32 = window.viewport_size().width.into();
        let mut leading = div().flex_1().min_w_0().flex().items_center();
        if route != &Route::Home {
            leading = leading.child(focus_ring(
                div()
                    .id("header-back")
                    .size(px(44.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .aria_label("Back")
                    .role(gpui::Role::Button)
                    .on_click(cx.listener(|shell, _, window, cx| shell.back(window, cx)))
                    .child(svg().path("back.svg").size(px(22.0)).text_color(theme.ink)),
                &self.back,
                theme,
                active,
                keyboard_navigation,
                window,
            ));
        }
        div()
            .w(px(content_width(width)))
            .mx_auto()
            .pt(px(20.0))
            .min_h(px(40.0))
            .flex()
            .items_center()
            .gap(px(16.0))
            .child(leading)
            .child(focus_ring(
                div()
                    .id("header-home")
                    .w(px(154.0))
                    .h(px(44.0))
                    .p(px(8.0))
                    .flex_shrink_0()
                    .cursor_pointer()
                    .aria_label("Panorama home")
                    .role(gpui::Role::Link)
                    .on_click(
                        cx.listener(|shell, _, window, cx| shell.navigate(Route::Home, window, cx)),
                    )
                    .child(
                        svg()
                            .path("logo.svg")
                            .w(px(138.0))
                            .h(px(28.0))
                            .text_color(theme.ink),
                    ),
                &self.home,
                theme,
                active,
                keyboard_navigation,
                window,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .justify_end()
                    .items_center()
                    .child(focus_ring(
                        div()
                            .id("header-account")
                            .caption()
                            .min_w(px(44.0))
                            .min_h(px(44.0))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .aria_label("Sign in")
                            .role(gpui::Role::Button)
                            .on_click(|_, _, _| {})
                            .child("Sign in"),
                        &self.account,
                        theme,
                        active,
                        keyboard_navigation,
                        window,
                    )),
            )
    }
}
