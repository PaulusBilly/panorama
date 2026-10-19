use super::*;
use crate::{
    playback::model::{Stage, Status},
    theme::{Theme, Typography, focus_ring},
};
use gpui::{Div, Transformation, div, prelude::*, px, svg};

impl Player {
    pub(super) fn overlays(
        &self,
        theme: Theme,
        reduced: bool,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let center = div()
            .absolute()
            .inset_0()
            .px(px(20.0))
            .flex()
            .items_center()
            .justify_center();
        match self.model.status {
            Status::Preparing(_) | Status::Buffering => {
                let label = if self.model.status == Status::Buffering {
                    "Buffering"
                } else if matches!(self.model.status, Status::Preparing(Stage::LoadingVideo)) {
                    "Loading video"
                } else {
                    "Resolving source"
                };
                center.child(
                    div()
                        .id("player-buffering")
                        .body()
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .py(px(12.0))
                        .px(px(20.0))
                        .bg(theme.player_canvas.opacity(0.75))
                        .role(gpui::Role::Status)
                        .aria_label(label)
                        .child(
                            svg()
                                .path("player-loader-2.svg")
                                .size(px(22.0))
                                .text_color(theme.player_ink)
                                .when(!reduced, |icon| {
                                    icon.with_transformation(Transformation::rotate(gpui::radians(
                                        self.launched.elapsed().as_secs_f32()
                                            * std::f32::consts::TAU,
                                    )))
                                }),
                        )
                        .child(label),
                )
            }
            Status::Error(error) => center.bg(theme.player_canvas.opacity(0.75)).child(
                div()
                    .id("player-error")
                    .max_w(px(512.0))
                    .text_center()
                    .role(gpui::Role::Alert)
                    .child(div().heading().child(error.message()))
                    .child(self.buttons(false, theme, keyboard, window, cx)),
            ),
            Status::Ended => center.bg(theme.player_canvas.opacity(0.7)).child(
                div()
                    .text_center()
                    .child(div().title().child("Playback ended"))
                    .child(self.buttons(true, theme, keyboard, window, cx)),
            ),
            _ => div(),
        }
    }

    fn buttons(
        &self,
        ended: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut row = div()
            .mt(px(24.0))
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(px(12.0));
        let focus_theme = Theme {
            focus_inner: theme.player_canvas,
            focus_outer: theme.player_ink,
            ..theme
        };
        for (index, label) in if ended {
            ["Replay", "Back to details"]
        } else {
            ["Retry playback", "Back to film"]
        }
        .into_iter()
        .enumerate()
        {
            row = row.child(focus_ring(
                div()
                    .id(label)
                    .role(gpui::Role::Button)
                    .aria_label(label)
                    .min_h(px(44.0))
                    .px(px(20.0))
                    .border_1()
                    .text_size(px(14.0))
                    .border_color(theme.player_ink.opacity(if index == 0 { 1.0 } else { 0.6 }))
                    .when(index == 0, |button| {
                        button.bg(theme.player_ink).text_color(theme.player_canvas)
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |player, _, window, cx| {
                        if index == 1 {
                            player.close(window, cx);
                        } else if ended {
                            player.send(Command::Seek(0.0), cx);
                            player.send(Command::SetPaused(false), cx);
                        } else {
                            player.send(Command::Retry, cx);
                        }
                    }))
                    .child(label),
                if index == 0 {
                    &self.primary_focus
                } else {
                    &self.secondary_focus
                },
                focus_theme,
                true,
                keyboard,
                window,
            ));
        }
        row
    }
}
