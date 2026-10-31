use super::*;
use crate::alert_row::AlertRow;
use gpui::Div;

impl Home {
    pub(super) fn alert(&self, message: String, label: &'static str, view: ViewSettings) -> Div {
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
        .render(move |_, _, cx| {
            state.update(cx, |state, cx| {
                if label == "Reload Panorama" {
                    state.reload(cx)
                } else {
                    state.load(false, cx)
                }
            });
        })
    }
}
