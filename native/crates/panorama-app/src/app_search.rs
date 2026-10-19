use super::*;
use crate::theme::{content_width, focus_ring};
use gpui::{Div, Focusable, relative, svg};
use gpui_component::input::Input;

impl AppShell {
    /// Toggle the shared search disclosure and focus its editing control.
    pub fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.history.current(), Route::Home | Route::Search { .. }) {
            return;
        }
        self.search.toggle(self.history.current());
        self.search_disclosures
            .insert(self.history.current_id(), self.search.open);
        self.menu.dismiss();
        if self.search.open {
            self.search_input.update(cx, |input, cx| {
                input.set_value(self.search.query.clone(), window, cx)
            });
            self.search_input.focus_handle(cx).focus(window, cx);
            self.sticky.hidden = false;
            self.header_slide
                .retarget(0.0, HEADER_SLIDE, Some(EASE_IN_OUT), Instant::now());
        } else {
            self.focus_header(window, cx);
        }
        self.search_caret(cx);
        cx.notify();
    }
    /// Clear the query and replace the current search destination with Home.
    pub fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let route = self.search.clear();
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.search_caret(cx);
        if self.history.replace(route) {
            self.changed(window, cx);
        }
        self.focus_header(window, cx);
        cx.notify();
    }
    pub(super) fn submit_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.search_input.read(cx).value().to_string();
        let route = self.search.submit(&value);
        if route == Route::Home {
            self.clear_search(window, cx);
            return;
        }
        let changed = if matches!(self.history.current(), Route::Search { .. }) {
            self.history.replace(route)
        } else {
            self.history.push(route)
        };
        if changed {
            self.changed(window, cx);
        }
        self.search_input.focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    pub(super) fn search_caret(&self, cx: &mut Context<Self>) {
        let caret = if self.search.open && self.login.is_none() {
            self.theme.inverse
        } else {
            self.theme.ink
        };
        gpui_component::Theme::update(cx, |theme| theme.colors.caret = caret.into());
    }
    /// Current full header height, including the animated disclosure.
    pub(crate) fn search_header_height(&self, width: f32) -> f32 {
        60.0 + self.search.height(width, self.reduced_motion)
            + if matches!(self.history.current(), Route::Search { .. }) {
                20.0
            } else {
                0.0
            }
    }
    pub(crate) fn home_search_padding(&self, width: f32) -> f32 {
        let full = 60.0
            + if width <= 700.0 {
                (width * 0.08 + 12.0).clamp(40.0, 48.0) * 1.2 + 73.0
            } else {
                (width * 0.05 + 12.0).clamp(48.0, 64.0) * 1.2 + 93.0
            };
        full * self
            .search
            .motion
            .value(Instant::now(), self.reduced_motion)
    }
    pub(super) fn render_search_band(
        &self,
        active: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let width = f32::from(window.viewport_size().width);
        let small = width <= 700.0;
        let size = if small {
            (width * 0.08 + 12.0).clamp(40.0, 48.0)
        } else {
            (width * 0.05 + 12.0).clamp(48.0, 64.0)
        };
        let enabled = active && self.search.open;
        let theme = self.theme;
        div()
            .h(px(self.search.height(width, self.reduced_motion)))
            .overflow_hidden()
            .child(
                div()
                    .w(px(content_width(width)))
                    .mx_auto()
                    .py(px(if small { 32.0 } else { 40.0 }))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .border_b_1()
                            .border_color(theme.inverse)
                            .pb(px(if small { 8.0 } else { 12.0 }))
                            .child(
                                Input::new(&self.search_input)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .aria_label("Search movies")
                                    .tab_index(if enabled { 0 } else { -1 })
                                    .disabled(!enabled)
                                    .map(|input| gpui::Styled::h(input, px(size * 1.2)))
                                    .p_0()
                                    .pr(px(if small { 64.0 } else { 80.0 }))
                                    .text_size(px(size))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .line_height(relative(1.2))
                                    .text_color(theme.inverse),
                            )
                            .child(
                                focus_ring(
                                    div()
                                        .id("search-submit")
                                        .size(px(52.0))
                                        .cursor_pointer()
                                        .role(gpui::Role::Button)
                                        .aria_label("Submit search")
                                        .on_click(cx.listener(|shell, _, window, cx| {
                                            shell.submit_search(window, cx)
                                        }))
                                        .child(
                                            svg()
                                                .path("arrow-right.svg")
                                                .size(px(52.0))
                                                .text_color(theme.inverse),
                                        ),
                                    &self.search_submit,
                                    theme,
                                    enabled,
                                    self.keyboard_navigation,
                                    window,
                                )
                                .absolute()
                                .right_0()
                                .bottom(px(if small {
                                    8.0
                                } else {
                                    12.0
                                })),
                            ),
                    ),
            )
    }
}
