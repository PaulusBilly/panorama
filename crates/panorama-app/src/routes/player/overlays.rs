use super::{panel::PanelId, widgets::focus_theme, *};
use crate::{
    playback::model::{Stage, Status},
    theme::{Theme, Typography, focus_ring},
};
use gpui::{Div, FocusHandle, Stateful, Transformation, div, prelude::*, px, svg};

const NO_PEERS_HEADING: &str = "No one is sharing this torrent right now.";
const NO_PEERS_BODY: &str =
    "It may start if a peer comes online. You can keep waiting or choose another source.";

/// The centre pause mark settles in over this long, then stays for as long as the film is paused.
const FLASH_IN: std::time::Duration = std::time::Duration::from_millis(180);
/// On resume the pause mark crossfades to the play mark over this long...
const FLASH_SWAP: std::time::Duration = std::time::Duration::from_millis(150);
/// ...which holds until this point...
const FLASH_HOLD: std::time::Duration = std::time::Duration::from_millis(500);
/// ...then swells and fades out, fully gone at this point.
const FLASH_OUT: std::time::Duration = std::time::Duration::from_millis(700);

impl Player {
    /// Whether the centre mark still needs frames: entering after a pause, or anywhere in the
    /// resume crossfade, hold and exit.
    pub(super) fn flash_moving(&self, now: Instant) -> bool {
        self.flash.is_some_and(|(play, at)| {
            now.duration_since(at) < if play { FLASH_OUT } else { FLASH_IN }
        })
    }

    /// The centre play/pause mark. Pausing settles a pause disc in and keeps it there while the
    /// film stays paused; resuming crossfades it to a play mark that holds, then swells as it
    /// fades. Reduced motion skips the scale and fades. Never takes clicks; the video layer below
    /// handles them.
    pub(super) fn pause_flash(&self, theme: Theme, reduced: bool, now: Instant) -> Option<Div> {
        let (play, at) = self.flash?;
        let elapsed = now.duration_since(at);
        let ease_out = |t: f32| 1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3);
        let ratio = |from: std::time::Duration, to: std::time::Duration| {
            (elapsed.saturating_sub(from).as_secs_f32() / (to - from).as_secs_f32()).clamp(0.0, 1.0)
        };
        // (disc opacity, disc size, how far the icon has turned from pause to play)
        let (opacity, size, swap) = if play {
            if elapsed >= FLASH_OUT {
                return None;
            }
            let out = ease_out(ratio(FLASH_HOLD, FLASH_OUT));
            let swap = ease_out(ratio(std::time::Duration::ZERO, FLASH_SWAP));
            if reduced {
                (1.0, 72.0, 1.0)
            } else {
                (1.0 - out, 72.0 + 12.0 * out, swap)
            }
        } else {
            // A resume from outside the toggle (a new source, the end of the film) clears the mark;
            // the short grace covers the host confirming the pause a frame or two late.
            if !self.model.paused && elapsed >= FLASH_IN {
                return None;
            }
            let enter = ease_out(ratio(std::time::Duration::ZERO, FLASH_IN));
            if reduced {
                (1.0, 72.0, 0.0)
            } else {
                (enter, 64.0 + 8.0 * enter, 0.0)
            }
        };
        let icon = |path: &'static str, opacity: f32| {
            svg()
                .absolute()
                .path(path)
                .size(px(size * 0.4))
                .opacity(opacity)
                .text_color(theme.ink)
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .relative()
                        .size(px(size))
                        .rounded_full()
                        .bg(theme.player_canvas.opacity(0.35))
                        .opacity(opacity)
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(swap < 1.0, |disc| {
                            disc.child(icon("player-pause-sharp.svg", 1.0 - swap))
                        })
                        .when(swap > 0.0, |disc| {
                            disc.child(icon("player-play-sharp.svg", swap))
                        }),
                ),
        )
    }

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
                } else if self.model.falling_back {
                    "Trying another source".into()
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
            // The Up Next card speaks for the end while it counts down.
            Status::Ended if self.counting_down() => div(),
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
            0.0,
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
        let first = if ended && self.next.is_some() {
            self.text_button(
                ("Next episode", true, &self.focus.retry),
                theme,
                keyboard,
                window,
                |player, _, cx| player.play_next(cx),
                cx,
            )
        } else if ended {
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
                |player, _, cx| player.retry(cx),
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
