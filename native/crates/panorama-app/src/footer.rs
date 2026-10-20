//! The wordmark band that closes Home and Addons.
use crate::theme::{Column, Theme};
use gpui::{Div, div, prelude::*, px, svg};

/// Render the footer for a viewport width, in its content column.
pub fn footer(theme: Theme, viewport: f32) -> Div {
    let col = Column::at(viewport);
    let container = col.width;
    div().w_full().bg(theme.surface).child(
        div()
            .w(px(container))
            .ml(px(col.left))
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
