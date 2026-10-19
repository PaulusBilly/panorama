//! Search placeholder with independent scroll state.
use crate::theme::{Typography, content_width};
use gpui::{Context, Render, ScrollHandle, Window, div, prelude::*, px};

/// Search placeholder with a retained scroll position.
pub struct Search {
    scroll: ScrollHandle,
    query: String,
}

impl Search {
    /// Create one history entry's independent view.
    pub fn new(query: String) -> Self {
        Self {
            scroll: ScrollHandle::new(),
            query,
        }
    }
}

impl Render for Search {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let width: f32 = window.viewport_size().width.into();
        div()
            .id("search-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .w(px(content_width(width)))
                    .mx_auto()
                    .pt(px(48.0))
                    .child(div().title().child("Search"))
                    .child(div().body().mt(px(12.0)).child(if self.query.is_empty() {
                        "Search for films.".into()
                    } else {
                        format!("Search for films. {}", self.query)
                    })),
            )
    }
}
