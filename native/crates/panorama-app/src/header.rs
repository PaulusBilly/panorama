use crate::{
    app::AppShell,
    router::Route,
    theme::{Theme, content_width, focus_ring},
};
use gpui::{Context, Div, FocusHandle, Window, div, prelude::*, px, svg};

/// Retained focus for the shared three-column header.
pub struct Header {
    back: FocusHandle,
    home: FocusHandle,
    /// Account trigger focus restored after sign-in.
    pub account: FocusHandle,
}

/// Sampled header appearance supplied by the shell's retained timelines.
pub struct HeaderAppearance {
    pub(crate) theme: Theme,
    pub(crate) active: bool,
    pub(crate) keyboard: bool,
    pub(crate) foreground: gpui::Rgba,
    pub(crate) logo: gpui::Rgba,
    pub(crate) background: gpui::Rgba,
    pub(crate) y: f32,
    pub(crate) pressed: f32,
    pub(crate) chrome_opacity: f32,
    pub(crate) search_open: bool,
}
impl Header {
    /// Allocate header controls once for each history entry.
    pub fn new(cx: &mut Context<AppShell>) -> Self {
        Self {
            back: cx.focus_handle(),
            home: cx.focus_handle(),
            account: cx.focus_handle(),
        }
    }
    /// Focus the leading route control.
    pub(crate) fn focus_search(&self, window: &mut Window, cx: &mut Context<AppShell>) {
        self.back.focus(window, cx);
    }
    /// Render fixed header chrome with sampled Home colors and translation.
    pub fn render(
        &self,
        route: &Route,
        appearance: HeaderAppearance,
        window: &Window,
        cx: &mut Context<AppShell>,
    ) -> Div {
        let width = f32::from(window.viewport_size().width);
        let HeaderAppearance {
            theme,
            active,
            keyboard,
            foreground,
            logo,
            background,
            y,
            pressed,
            chrome_opacity,
            search_open,
        } = appearance;
        let home_route = matches!(route, Route::Home | Route::Search { .. });
        div()
            .absolute()
            .top(px(y))
            .left_0()
            .w_full()
            .h(px(60.0))
            .bg(background)
            .text_color(foreground)
            .child(
                div()
                    .w(px(content_width(width)))
                    .mx_auto()
                    .pt(px(20.0))
                    .h(px(60.0))
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .child(focus_ring(
                                div()
                                    .id("header-back")
                                    .w(px(22.0))
                                    .h(px(28.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .aria_label(if home_route {
                                        if search_open {
                                            "Close search"
                                        } else {
                                            "Search"
                                        }
                                    } else {
                                        "Back to films"
                                    })
                                    .role(gpui::Role::Button)
                                    .aria_expanded(search_open)
                                    .on_click(cx.listener(move |shell, _, window, cx| {
                                        if home_route {
                                            shell.toggle_search(window, cx);
                                        } else {
                                            shell.back(window, cx);
                                        }
                                    }))
                                    .child(
                                        svg()
                                            .path(if home_route {
                                                if search_open { "x.svg" } else { "search.svg" }
                                            } else {
                                                "back.svg"
                                            })
                                            .size(px(22.0))
                                            .text_color(foreground),
                                    ),
                                &self.back,
                                theme,
                                active,
                                keyboard,
                                window,
                            )),
                    )
                    .child(focus_ring(
                        div()
                            .id("header-home")
                            .w(px(138.0))
                            .h(px(28.0))
                            .opacity(chrome_opacity)
                            .flex_shrink_0()
                            .cursor_pointer()
                            .aria_label("Panorama home")
                            .role(gpui::Role::Link)
                            .on_click(cx.listener(|shell, _, window, cx| {
                                if !shell.search.open {
                                    shell.navigate(Route::Home, window, cx);
                                }
                            }))
                            .child(
                                svg()
                                    .path("logo.svg")
                                    .w(px(138.0))
                                    .h(px(28.0))
                                    .text_color(logo),
                            ),
                        &self.home,
                        theme,
                        active && !search_open,
                        keyboard,
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
                                    .size(px(40.0))
                                    .opacity(chrome_opacity)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .aria_label("Account menu")
                                    .role(gpui::Role::Button)
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(|shell, _, _, cx| {
                                            shell.press_trigger(true);
                                            cx.notify();
                                        }),
                                    )
                                    .on_mouse_up(
                                        gpui::MouseButton::Left,
                                        cx.listener(|shell, _, _, cx| {
                                            shell.press_trigger(false);
                                            cx.notify();
                                        }),
                                    )
                                    .on_mouse_up_out(
                                        gpui::MouseButton::Left,
                                        cx.listener(|shell, _, _, cx| {
                                            shell.press_trigger(false);
                                            cx.notify();
                                        }),
                                    )
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        if !shell.search.open {
                                            shell.menu.set_open(!shell.menu.open);
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        svg()
                                            .path("menu-2.svg")
                                            .size(px(22.0 * pressed))
                                            .text_color(foreground),
                                    ),
                                &self.account,
                                theme,
                                active && !search_open,
                                keyboard,
                                window,
                            )),
                    ),
            )
    }
}
