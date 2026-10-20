//! The top bar (close, volume, fullscreen) and the bottom bar (title, panels, seek).
use super::{
    Player,
    measure::probe,
    panel::PanelId,
    range::{self, RangeId},
    range_view::RangeView,
    time,
    widgets::{Button, Control, Fill, focus_theme, pill, ring, tabular},
};
use crate::assets::Weighted;
use crate::theme::{Theme, Typography, focus_ring};
use gpui::{
    Context, Div, FontWeight, Window, div, linear_color_stop, linear_gradient, prelude::*, px,
};

/// The macOS title-bar row the video runs under; the top bar starts below it so the
/// close button clears the traffic lights and the drag strip covers only the gradient.
const TRAFFIC_LIGHTS_ROW: f32 = 28.0;

fn traffic_lights(window: &Window) -> bool {
    cfg!(target_os = "macos") && !window.is_fullscreen()
}

/// Below this window width the volume range hides (`max-[540px]:hidden`).
const VOLUME_HIDDEN_AT: f32 = 540.0;

impl Player {
    pub(super) fn top_bar(
        &self,
        theme: Theme,
        keyboard: bool,
        fade: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let close = self.icon_button(
            Button {
                control: Control::Close,
                id: "player-close",
                label: "Close",
                icon: "player-x.svg",
                size: 40.0,
                icon_size: 20.0,
                fill: Fill::Pill,
                on: false,
                disabled: false,
            },
            theme,
            keyboard,
            window,
            cx,
            |player, window, cx| player.close(window, cx),
        );
        let mut bar = div()
            .absolute()
            .left_0()
            .right_0()
            .top_0()
            .px(px(20.0))
            .pt(px(
                16.0 + TRAFFIC_LIGHTS_ROW * f32::from(traffic_lights(window))
            ))
            .pb(px(48.0))
            .opacity(fade)
            .bg(linear_gradient(
                180.0,
                linear_color_stop(theme.player_canvas.opacity(0.7), 0.0),
                linear_color_stop(theme.player_canvas.opacity(0.0), 1.0),
            ))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(close);
        if matches!(
            self.model.status,
            crate::playback::model::Status::Ready | crate::playback::model::Status::Buffering
        ) {
            bar = bar.child(self.top_right(theme, keyboard, window, cx));
        }
        bar
    }

    fn top_right(
        &self,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = self.model.muted;
        let wide = f32::from(window.viewport_size().width) > VOLUME_HIDDEN_AT;
        let mute = self.icon_button(
            Button {
                control: Control::Mute,
                id: "player-mute",
                label: if muted { "Unmute" } else { "Mute" },
                icon: if muted {
                    "volume-off.svg"
                } else {
                    "volume-2.svg"
                },
                size: 36.0,
                icon_size: 20.0,
                fill: Fill::None,
                on: muted,
                disabled: false,
            },
            theme,
            keyboard,
            window,
            cx,
            |player, _, cx| {
                let muted = !player.model.muted;
                player.model.muted = muted;
                player.send(crate::playback::host::Command::SetMuted(muted), cx);
                cx.notify();
            },
        );
        let volume = if muted {
            0.0
        } else {
            f32::from(self.model.volume) / 100.0
        };
        let fullscreen = self.model.fullscreen;
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                pill(theme)
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .p(px(2.0))
                    .occlude()
                    .child(mute)
                    .when(wide, |pill| {
                        pill.child(div().w(px(96.0)).mr(px(8.0)).child(self.range(
                            RangeView {
                                id: RangeId::Volume,
                                label: "Volume",
                                played: volume,
                                buffered: volume,
                                disabled: false,
                            },
                            theme,
                            keyboard,
                            window,
                            cx,
                        )))
                    })
                    .child(ring(theme, 0.12)),
            )
            .child(div().occlude().rounded_full().child(self.icon_button(
                Button {
                    control: Control::Fullscreen,
                    id: "player-fullscreen",
                    label: if fullscreen {
                        "Exit fullscreen"
                    } else {
                        "Fullscreen"
                    },
                    icon: if fullscreen {
                        "minimize-player.svg"
                    } else {
                        "maximize-player.svg"
                    },
                    size: 40.0,
                    icon_size: 20.0,
                    fill: if fullscreen { Fill::Active } else { Fill::Pill },
                    on: false,
                    disabled: false,
                },
                theme,
                keyboard,
                window,
                cx,
                |player, window, cx| player.toggle_fullscreen(window, cx),
            )))
    }

    fn trigger(
        &self,
        panel: PanelId,
        (id, label, icon, open_icon): (&'static str, &'static str, &'static str, &'static str),
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.panels.active() == Some(panel);
        self.icon_button(
            Button {
                control: Control::trigger(panel),
                id,
                label,
                icon: if open { open_icon } else { icon },
                size: 36.0,
                icon_size: 20.0,
                fill: Fill::None,
                on: open,
                disabled: false,
            },
            theme,
            keyboard,
            window,
            cx,
            move |player, window, cx| player.toggle_panel(panel, window, cx),
        )
    }

    fn play_button(
        &self,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let paused = self.model.paused;
        let handle = &self.focus.controls[Control::Play as usize];
        if !handle.is_focused(window) {
            return div().absolute().size(px(1.0)).child(focus_ring(
                div()
                    .id("player-play")
                    .role(gpui::Role::Button)
                    .aria_label(if paused { "Play" } else { "Pause" })
                    .absolute()
                    .size(px(1.0))
                    .opacity(0.0)
                    .on_click(cx.listener(|player, _, _, cx| {
                        player.send(crate::playback::host::Command::TogglePause, cx);
                    })),
                handle,
                focus_theme(theme),
                true,
                keyboard,
                window,
            ));
        }
        self.icon_button(
            Button {
                control: Control::Play,
                id: "player-play",
                label: if paused { "Play" } else { "Pause" },
                icon: if paused {
                    "player-play-filled.svg"
                } else {
                    "player-pause-filled.svg"
                },
                size: 28.0,
                icon_size: 16.0,
                fill: Fill::None,
                on: false,
                disabled: false,
            },
            theme,
            keyboard,
            window,
            cx,
            |player, _, cx| player.send(crate::playback::host::Command::TogglePause, cx),
        )
    }

    pub(super) fn bottom_bar(
        &self,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let shown = self.scrub.shown(self.model.time.unwrap_or(0.0));
        let (played, buffered) =
            range::fractions(self.model.duration, shown, self.model.buffered_until);
        let label = |text: String| {
            div()
                .flex_shrink_0()
                .caption()
                .weight(FontWeight::MEDIUM)
                .font_features(tabular())
                .text_color(theme.player_ink.opacity(0.65))
                .child(text)
        };
        let triggers = pill(theme)
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(2.0))
            .p(px(2.0))
            .occlude()
            .child(probe(&self.measures, |measures, bounds| {
                measures.pill.set(Some(bounds));
            }))
            .child(self.trigger(
                PanelId::Subtitles,
                (
                    "player-subtitles",
                    "Subtitles",
                    if self.model.subtitles.iter().any(|track| track.selected) {
                        "badge-cc-filled.svg"
                    } else {
                        "badge-cc.svg"
                    },
                    "badge-cc-filled.svg",
                ),
                theme,
                keyboard,
                window,
                cx,
            ))
            .child(self.trigger(
                PanelId::Audio,
                (
                    "player-audio",
                    "Audio",
                    "playlist.svg",
                    "playlist-filled.svg",
                ),
                theme,
                keyboard,
                window,
                cx,
            ))
            .child(self.trigger(
                PanelId::Sources,
                (
                    "player-sources",
                    "Sources",
                    "list-details.svg",
                    "list-details-filled.svg",
                ),
                theme,
                keyboard,
                window,
                cx,
            ))
            .child(self.trigger(
                PanelId::Settings,
                (
                    "player-settings",
                    "Playback settings",
                    "settings.svg",
                    "settings.svg",
                ),
                theme,
                keyboard,
                window,
                cx,
            ))
            .child(ring(theme, 0.12));
        let seek = pill(theme)
            .mt(px(12.0))
            .h(px(36.0))
            .px(px(16.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .occlude()
            .child(probe(&self.measures, |measures, bounds| {
                measures.seek_pill.set(Some(bounds));
            }))
            .child(self.play_button(theme, keyboard, window, cx))
            .child(label(time::format_time(shown)))
            .child(div().flex_1().min_w_0().child(self.range(
                RangeView {
                    id: RangeId::Seek,
                    label: "Playback position",
                    played,
                    buffered,
                    disabled: self.model.duration.is_none(),
                },
                theme,
                keyboard,
                window,
                cx,
            )))
            .child(label(time::remaining(self.model.duration, shown)))
            .child(ring(theme, 0.12));
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .px(px(20.0))
            .pb(px(18.0))
            .pt(px(80.0))
            .bg(linear_gradient(
                0.0,
                linear_color_stop(theme.player_canvas.opacity(0.8), 0.0),
                linear_color_stop(theme.player_canvas.opacity(0.0), 1.0),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(24.0))
                    .child(
                        div()
                            .min_w_0()
                            .title()
                            .weight(FontWeight::BOLD)
                            .truncate()
                            .child(self.model.title.clone()),
                    )
                    .child(triggers),
            )
            .child(seek)
    }
}
