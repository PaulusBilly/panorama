use super::*;
use crate::alert_row::AlertRow;
use gpui::Div;

impl Home {
    pub(super) fn alert(
        &self,
        message: String,
        label: &'static str,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = self.state.clone();
        AlertRow {
            message,
            label,
            theme: view.theme,
            width: view.width,
            focus: if label == "Reload Panorama" {
                &self.reload
            } else {
                &self.retry
            },
            active: view.active,
            keyboard: view.keyboard,
        }
        .render(
            window,
            cx.listener(move |home, _, window, cx| {
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
            }),
        )
    }
}
