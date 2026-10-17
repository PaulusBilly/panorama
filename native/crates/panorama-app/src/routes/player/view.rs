use super::*;
use crate::{
    playback::model::Status,
    theme::{Theme, focus_ring},
};
use gpui::{
    Div, MouseButton, Render, div, linear_color_stop, linear_gradient, prelude::*, px, svg,
};
use std::time::Duration;

impl Render for Player {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (active, theme, reduced, keyboard) = self
            .shell
            .upgrade()
            .map(|shell| {
                let shell = shell.read(cx);
                (
                    shell.is_current(self.entry),
                    shell.theme,
                    shell.reduced_motion,
                    shell.keyboard_navigation,
                )
            })
            .unwrap_or((false, Theme::default(), true, false));
        if !active {
            self.unmount(cx);
            return div();
        }
        self.mount(window, cx);
        self.sync_surface(window, cx);
        let now = Instant::now();
        let preparing = self.model.initial_preparing();
        let fixture_canvas = self.args.fixtures
            && self.args.play_file.is_none()
            && self.args.play_url.is_none()
            && !self.args.play_torrent;
        let animated =
            preparing || matches!(self.model.status, Status::Preparing(_) | Status::Buffering);
        if (!reduced && animated)
            || self.press.moving(now, reduced)
            || self.hover.moving(now, reduced)
        {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        div()
            .relative()
            .size_full()
            .text_color(theme.player_ink)
            .when(
                fixture_canvas || matches!(self.model.status, Status::Idle | Status::Error(_)),
                |root| root.bg(theme.player_canvas),
            )
            .child(
                div()
                    .id("player-video")
                    .absolute()
                    .inset_0()
                    .on_click(cx.listener(|player, _, _, cx| {
                        if matches!(player.model.status, Status::Ready | Status::Buffering) {
                            player.send(Command::TogglePause, cx);
                        }
                    })),
            )
            .when(preparing, |root| {
                root.child(
                    div()
                        .id("player-initial-preparing")
                        .absolute()
                        .inset_0()
                        .bg(theme.player_canvas)
                        .flex()
                        .items_center()
                        .justify_center()
                        .role(gpui::Role::Status)
                        .aria_label("Preparing playback")
                        .child(crate::cascade_loader::render(
                            now.duration_since(self.launched).as_secs_f32(),
                            reduced,
                            theme,
                        )),
                )
            })
            .when(!preparing, |root| {
                root.child(self.overlays(theme, reduced, keyboard, window, cx))
                    .child(self.top_bar(theme, reduced, keyboard, window, cx))
            })
    }
}

impl Player {
    fn top_bar(
        &self,
        theme: Theme,
        reduced: bool,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let scale = self.press.value(Instant::now(), reduced);
        let hover = self.hover.value(Instant::now(), reduced);
        let focus_theme = Theme {
            focus_inner: theme.player_canvas,
            focus_outer: theme.player_ink,
            ..theme
        };
        let close = focus_ring(
            div()
                .id("player-close")
                .role(gpui::Role::Button)
                .aria_label("Close")
                .size(px(40.0 * scale))
                .rounded_full()
                .border_1()
                .border_color(theme.player_ink.opacity(0.12))
                .bg(theme.player_canvas.opacity(0.72))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .on_hover(cx.listener(|player, over, _, cx| {
                    player.hover.retarget(
                        if *over { 1.0 } else { 0.0 },
                        Duration::from_millis(120),
                        Some(crate::theme::motion::EASE_OUT),
                        Instant::now(),
                    );
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|player, _, _, cx| {
                        player.press.retarget(
                            0.97,
                            Duration::from_millis(120),
                            Some(crate::theme::motion::EASE_OUT),
                            Instant::now(),
                        );
                        cx.notify();
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|player, _, _, cx| {
                        player.press.retarget(
                            1.0,
                            Duration::from_millis(120),
                            Some(crate::theme::motion::EASE_OUT),
                            Instant::now(),
                        );
                        cx.notify();
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|player, _, _, cx| {
                        player.press.retarget(
                            1.0,
                            Duration::from_millis(120),
                            Some(crate::theme::motion::EASE_OUT),
                            Instant::now(),
                        );
                        cx.notify();
                    }),
                )
                .on_click(cx.listener(|player, _, window, cx| player.close(window, cx)))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded_full()
                        .bg(theme.player_ink.opacity(0.1 * hover)),
                )
                .child(
                    svg()
                        .path("player-x.svg")
                        .size(px(20.0 * scale))
                        .text_color(theme.player_ink),
                ),
            &self.close_focus,
            focus_theme,
            true,
            keyboard,
            window,
        );
        div()
            .absolute()
            .left_0()
            .right_0()
            .top_0()
            .px(px(20.0))
            .pt(px(16.0))
            .pb(px(48.0))
            .bg(linear_gradient(
                180.0,
                linear_color_stop(theme.player_canvas.opacity(0.7), 0.0),
                linear_color_stop(theme.player_canvas.opacity(0.0), 1.0),
            ))
            .child(
                div()
                    .size(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(close),
            )
    }
}
