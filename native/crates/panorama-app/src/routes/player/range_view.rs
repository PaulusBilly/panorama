//! Painting of the seek and volume ranges (`.player-range`).
use super::{
    Player,
    measure::probe,
    range::{HIT_HEIGHT, RangeId},
    widgets::focus_theme,
};
use crate::theme::{Theme, focus_ring};
use gpui::{
    Context, Div, MouseButton, MouseDownEvent, Rgba, Window, div, prelude::*, px, relative,
};

/// What one range shows.
pub(super) struct RangeView {
    pub id: RangeId,
    pub label: &'static str,
    /// Played fraction, 0..=1.
    pub played: f32,
    /// Buffered fraction, never below played.
    pub buffered: f32,
    pub disabled: bool,
}

impl Player {
    pub(super) fn range(
        &self,
        view: RangeView,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = view.id;
        let look = id.look();
        let ink = theme.player_ink;
        let emphasis = self.emphasis[id as usize].value(std::time::Instant::now(), self.reduced);
        let height = look.track_height(emphasis);
        let cap = look.thumb / 2.0;
        // Fill segments are disjoint, like the CSS gradient's hard stops, so translucent
        // colors never blend over each other.
        let segment = |from: f32, to: f32, color: Rgba| {
            div()
                .absolute()
                .left(px(cap))
                .right(px(cap))
                .top_0()
                .bottom_0()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(from))
                        .w(relative((to - from).max(0.0)))
                        .bg(color),
                )
        };
        let end_cap = |left: bool, color: Rgba| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .w(px(cap))
                .when(left, |block| block.left_0())
                .when(!left, |block| block.right_0())
                .bg(color)
        };
        let (rest, buffered_color) = (ink.opacity(0.18), ink.opacity(0.4));
        let right_cap = if view.played >= 1.0 {
            ink
        } else if view.buffered >= 1.0 {
            buffered_color
        } else {
            rest
        };
        let track = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px((HIT_HEIGHT - height) / 2.0))
            .h(px(height))
            .rounded_full()
            .overflow_hidden()
            .child(end_cap(true, ink))
            .child(segment(0.0, view.played, ink))
            .child(segment(view.played, view.buffered, buffered_color))
            .child(segment(view.buffered, 1.0, rest))
            .child(end_cap(false, right_cap));
        let thumb = div()
            .absolute()
            .left_0()
            .right(px(look.thumb))
            .top_0()
            .bottom_0()
            .child(
                div()
                    .absolute()
                    .left(relative(view.played))
                    .top(px((HIT_HEIGHT - look.thumb) / 2.0))
                    .size(px(look.thumb))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(look.thumb * look.dot_scale(emphasis)))
                            .rounded_full()
                            .bg(ink)
                            .opacity(look.dot_opacity(emphasis)),
                    ),
            );
        let disabled = view.disabled;
        let control = div()
            .id(("player-range", id as usize))
            .role(gpui::Role::Slider)
            .aria_label(view.label)
            .w_full()
            .h(px(HIT_HEIGHT))
            .when(disabled, |range| range.opacity(0.5))
            .when(!disabled, |range| {
                range
                    .cursor_pointer()
                    .on_hover(cx.listener(move |player, over, _, cx| {
                        player.hovered[id as usize] = *over;
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |player, event: &MouseDownEvent, window, cx| {
                            player.range_press(id, event.position, window, cx);
                        }),
                    )
            })
            .child(probe(&self.measures, move |measures, bounds| {
                measures.ranges[id as usize].set(bounds);
            }))
            .child(track)
            .child(thumb);
        let handle = match id {
            RangeId::Volume => &self.focus.volume,
            RangeId::Seek => &self.focus.seek,
        };
        div().w_full().child(focus_ring(
            control,
            handle,
            focus_theme(theme),
            !disabled,
            keyboard,
            window,
        ))
    }
}
