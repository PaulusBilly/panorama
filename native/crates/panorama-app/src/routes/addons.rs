//! Addons placeholder with independent scroll state.
use crate::theme::{Typography, content_width};
use gpui::{Context, Render, ScrollHandle, Window, div, prelude::*, px};

/// Addons placeholder with a retained scroll position.
pub struct Addons {
    scroll: ScrollHandle,
}

impl Addons {
    /// Create one history entry's independent view.
    pub fn new() -> Self {
        Self {
            scroll: ScrollHandle::new(),
        }
    }
}
impl Default for Addons {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Addons {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let width: f32 = window.viewport_size().width.into();
        div()
            .id("addons-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .w(px(content_width(width)))
                    .mx_auto()
                    .pt(px(48.0))
                    .child(div().title().child("Addons"))
                    .child(div().body().mt(px(12.0)).child("Manage your addons here.")),
            )
    }
}
