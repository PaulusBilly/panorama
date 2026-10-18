//! The wordmark band that closes Home and Addons.
use crate::theme::{Theme, content_width};
use gpui::{Div, div, prelude::*, px, svg};

/// Render the footer for a viewport width.
pub fn footer(theme: Theme, viewport: f32) -> Div {
    let container = content_width(viewport);
    div().w_full().bg(theme.surface).child(
        div()
            .w(px(container))
            .mx_auto()
            .pt(px(200.0 + (viewport * 0.12).clamp(72.0, 128.0)))
            .child(
                svg()
                    .path("panorama.svg")
                    .w(px(container))
                    .h(px(container * 207.0 / 1223.0))
                    .text_color(gpui::rgb(0xffffff)),
            ),
    )
}
