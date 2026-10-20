use super::{panel::PanelId, widgets::focus_theme, *};
use crate::{
    playback::model::{Stage, Status},
    theme::{Theme, Typography, focus_ring},
};
use gpui::{Div, FocusHandle, Stateful, Transformation, div, prelude::*, px, svg};

const NO_PEERS_HEADING: &str = "No one is sharing this torrent right now.";
const NO_PEERS_BODY: &str =
    "It may start if a peer comes online. You can keep waiting or choose another source.";

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
                let caption = self.model.torrent_caption();
                let label = if self.model.status == Status::Buffering {
                    self.health.buffering_label()
                } else if self.model.finding_peers() {
                    "Finding peers".into()
                } else if matches!(self.model.status, Status::Preparing(Stage::LoadingVideo)) {
                    "Loading video".into()
                } else {
                    "Resolving source".into()
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
                        .aria_label(match &caption {
                            Some(caption) => format!("{label}, {caption}"),
                            None => label.to_string(),
                        })
                        .child(
                            svg()
                                .path("player-loader-2.svg")
                                .size(px(22.0))
                                .text_color(theme.ink)
                                .when(!reduced, |icon| {
                                    icon.with_transformation(Transformation::rotate(gpui::radians(
                                        self.launched.elapsed().as_secs_f32()
                                            * std::f32::consts::TAU,
                                    )))
                                }),
                        )
                        .child(div().child(label).when_some(caption, |column, caption| {
                            column.child(
                                div()
                                    .caption()
                                    .text_color(theme.ink.opacity(0.65))
                                    .child(caption),
                            )
                        })),
                )
            }
            Status::NoPeers => center.bg(theme.player_canvas.opacity(0.75)).child(
                div()
                    .id("player-no-peers")
                    .max_w(px(512.0))
                    .text_center()
                    .role(gpui::Role::Alert)
                    .aria_label(NO_PEERS_HEADING)
                    .child(div().heading().child(NO_PEERS_HEADING))
                    .child(
                        div()
                            .body()
                            .mt(px(8.0))
                            .text_color(theme.ink.opacity(0.8))
                            .child(NO_PEERS_BODY),
                    )
                    .child(self.no_peers_buttons(theme, keyboard, window, cx)),
            ),
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

    fn text_button(
        &self,
        (label, primary, handle): (&'static str, bool, &FocusHandle),
        theme: Theme,
        keyboard: bool,
        window: &Window,
        on_click: impl Fn(&mut Player, &mut Window, &mut Context<Player>) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        focus_ring(
            div()
                .id(label)
                .role(gpui::Role::Button)
                .aria_label(label)
                .min_h(px(44.0))
                .px(px(20.0))
                .border_1()
                .text_size(px(14.0))
                .border_color(theme.ink.opacity(if primary { 1.0 } else { 0.6 }))
                .when(primary, |button| {
                    button.bg(theme.ink).text_color(theme.player_canvas)
                })
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_click(cx.listener(move |player, _, window, cx| on_click(player, window, cx)))
                .child(label),
            handle,
            focus_theme(theme),
            true,
            keyboard,
            window,
        )
    }

    /// Under the cascade loader of a torrent's first preparation: what it is waiting for.
    pub(super) fn torrent_preparing_text(&self, theme: Theme) -> Div {
        let label = if self.model.finding_peers() {
            "Finding peers"
        } else {
            "Loading video"
        };
        div()
            .absolute()
            .top(px(110.0))
            .left(px(-144.0))
            .w(px(480.0))
            .text_center()
            .child(div().body().child(label))
            .when_some(self.model.torrent_caption(), |text, caption| {
                text.child(
                    div()
                        .caption()
                        .text_color(theme.ink.opacity(0.65))
                        .child(caption),
                )
            })
    }

    fn no_peers_buttons(
        &self,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let choose = self.text_button(
            ("Choose another source", true, &self.focus.retry),
            theme,
            keyboard,
            window,
            |player, window, cx| player.show_panel(PanelId::Sources, window, cx),
            cx,
        );
        let wait = self.text_button(
            ("Keep waiting", false, &self.focus.back),
            theme,
            keyboard,
            window,
            |player, _, cx| player.send(Command::RetryTorrent, cx),
            cx,
        );
        div()
            .mt(px(24.0))
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(px(12.0))
            .child(choose)
            .child(wait)
    }

    fn buttons(
        &self,
        ended: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let first = if ended {
            self.text_button(
                ("Replay", true, &self.focus.retry),
                theme,
                keyboard,
                window,
                |player, _, cx| {
                    player.send(Command::Seek(0.0), cx);
                    player.send(Command::SetPaused(false), cx);
                },
                cx,
            )
        } else {
            self.text_button(
                ("Retry playback", true, &self.focus.retry),
                theme,
                keyboard,
                window,
                |player, _, cx| player.send(Command::Retry, cx),
                cx,
            )
        };
        let second = if ended {
            self.text_button(
                ("Back to details", false, &self.focus.back),
                theme,
                keyboard,
                window,
                |player, window, cx| player.close(window, cx),
                cx,
            )
        } else {
            self.text_button(
                ("Back to sources", false, &self.focus.back),
                theme,
                keyboard,
                window,
                |player, window, cx| player.show_panel(PanelId::Sources, window, cx),
                cx,
            )
        };
        div()
            .mt(px(24.0))
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(px(12.0))
            .child(first)
            .child(second)
    }

    /// Warn when the source outpaces the connection, once playback has stalled.
    pub(super) fn heavy_notice(
        &self,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !self.health.too_heavy
            || self.dismissed == Some(self.model.source)
            || !matches!(self.model.status, Status::Ready | Status::Buffering)
        {
            return None;
        }
        let copy = self.health.heavy_copy()?;
        let sources = self.text_button(
            ("Sources", true, &self.focus.notice_sources),
            theme,
            keyboard,
            window,
            |player, window, cx| player.show_panel(PanelId::Sources, window, cx),
            cx,
        );
        let dismiss = self.text_button(
            ("Dismiss", false, &self.focus.notice_dismiss),
            theme,
            keyboard,
            window,
            |player, _, cx| {
                player.dismissed = Some(player.model.source);
                cx.notify();
            },
            cx,
        );
        Some(
            div()
                .id("player-heavy-notice")
                .role(gpui::Role::Status)
                .absolute()
                .left_0()
                .right_0()
                .top(px(80.0))
                .px(px(20.0))
                .flex()
                .justify_center()
                .child(
                    div()
                        .occlude()
                        .max_w(px(672.0))
                        .body()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x(px(20.0))
                        .gap_y(px(12.0))
                        .bg(theme.player_canvas.opacity(0.85))
                        .px(px(20.0))
                        .py(px(16.0))
                        .child(div().min_w(px(288.0)).flex_1().child(copy))
                        .child(div().flex().gap(px(12.0)).child(sources).child(dismiss)),
                ),
        )
    }
}
