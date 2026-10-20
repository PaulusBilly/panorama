//! Canvas-coloured gradients that keep copy legible over any film still.
use super::Theme;
use gpui::{DefiniteLength, Div, Rgba, div, linear_color_stop, linear_gradient, prelude::*};

/// The parent edge a scrim darkens from.
#[derive(Clone, Copy, Debug)]
pub enum Edge {
    /// Darkest at the left, clearing to the right.
    Left,
    /// Darkest at the bottom, clearing upward.
    Bottom,
}

/// Darken `extent` of the parent inward from `edge`, `strength` being the opacity at the edge.
///
/// GPUI gradients take two stops, so the falloff is two stacked halves: `strength` to
/// `0.45 * strength`, then to clear. A single linear ramp leaves a visible shelf where it ends.
/// Every stop is the scrim colour at some alpha, never transparent black, so the interpolation
/// never passes through grey.
pub fn scrim(theme: Theme, edge: Edge, extent: impl Into<DefiniteLength>, strength: f32) -> Div {
    let extent = extent.into();
    let knee = 0.45 * strength;
    let tone = |alpha: f32| Rgba {
        a: alpha,
        ..theme.scrim
    };
    // GPUI follows CSS: 0deg runs bottom to top, 90deg left to right; the first stop is the edge.
    let (angle, frame) = match edge {
        Edge::Left => (90.0, div().left_0().top_0().h_full().w(extent).flex()),
        Edge::Bottom => (
            0.0,
            div()
                .left_0()
                .bottom_0()
                .w_full()
                .h(extent)
                .flex()
                .flex_col_reverse(),
        ),
    };
    let half = |from: f32, to: f32| {
        div().flex_1().bg(linear_gradient(
            angle,
            linear_color_stop(tone(from), 0.0),
            linear_color_stop(tone(to), 1.0),
        ))
    };
    frame
        .absolute()
        .child(half(strength, knee))
        .child(half(knee, 0.0))
}
