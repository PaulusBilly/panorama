//! Player placeholder without the shared header.
use crate::theme::{Typography, content_width};
use gpui::{Context, Render, ScrollHandle, Window, div, prelude::*, px};

/// Player placeholder with a retained scroll position.
pub struct Player {
    scroll: ScrollHandle,
    id: String,
    _source: Option<panorama_core::addons::StreamSource>,
}

impl Player {
    /// Create one history entry's independent view.
    pub fn new(id: String, source: Option<panorama_core::addons::StreamSource>) -> Self {
        Self {
            scroll: ScrollHandle::new(),
            id,
            _source: source,
        }
    }
}

impl Render for Player {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let width: f32 = window.viewport_size().width.into();
        div()
            .id("player-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .w(px(content_width(width)))
                    .mx_auto()
                    .pt(px(48.0))
                    .child(div().title().child("Player"))
                    .child(div().body().mt(px(12.0)).child(if self.id.is_empty() {
                        "Playback will appear here.".into()
                    } else {
                        format!("Playback will appear here. {}", self.id)
                    })),
            )
    }
}
