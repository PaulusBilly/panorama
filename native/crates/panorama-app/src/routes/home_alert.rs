use super::*;
use crate::theme::{Typography, focus_ring};
use gpui::{Div, div, relative};

impl Home {
    pub(super) fn alert(
        &self,
        message: String,
        label: &'static str,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let state = self.state.clone();
        div()
            .mt(px(32.0))
            .py(px(22.0))
            .border_y_1()
            .border_color(theme.rule)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(32.0))
            .when(view.width < 700.0, |row| row.flex_col().items_start())
            .child(div().body().child(message))
            .child(focus_ring(
                div()
                    .id(label)
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
                    .aria_label(label)
                    .on_click(cx.listener(move |home, _, window, cx| {
                        if home.search.is_some() {
                            if label == "Clear search" {
                                let _ = home
                                    .shell
                                    .update(cx, |shell, cx| shell.clear_search(window, cx));
                            } else {
                                home.load_search(cx);
                            }
                        } else {
                            state.update(cx, |state, cx| {
                                if label == "Reload Panorama" {
                                    state.reload(cx)
                                } else {
                                    state.load(false, cx)
                                }
                            });
                        }
                    }))
                    .child(label),
                if label == "Reload Panorama" {
                    &self.reload
                } else {
                    &self.retry
                },
                theme,
                view.active,
                view.keyboard,
                window,
            ))
    }
}
