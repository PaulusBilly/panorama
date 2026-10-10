//! Film placeholder with independent scroll state.
use crate::theme::{Typography, content_width};
use gpui::{Context, Render, ScrollHandle, Window, div, prelude::*, px};

/// Film placeholder with a retained scroll position.
pub struct Film {
    scroll: ScrollHandle,
    id: String,
}

impl Film {
    /// Create one history entry's independent view.
    pub fn new(id: String) -> Self {
        Self {
            scroll: ScrollHandle::new(),
            id,
        }
    }
}

impl Render for Film {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let width: f32 = window.viewport_size().width.into();
        div()
            .id("film-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .w(px(content_width(width)))
                    .mx_auto()
                    .pt(px(48.0))
                    .child(div().title().child("Film"))
                    .child(div().body().mt(px(12.0)).child(if self.id.is_empty() {
                        "Film details will appear here.".into()
                    } else {
                        format!("Film details will appear here. {}", self.id)
                    })),
            )
    }
}
