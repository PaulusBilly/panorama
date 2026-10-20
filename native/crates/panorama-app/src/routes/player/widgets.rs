//! Shared building blocks of the player chrome: control motion, focus handles, pills and buttons.
use super::{Player, panel::PanelId};
use crate::{
    theme::{Theme, focus_ring, motion::EASE_OUT},
    transition::Tween,
};
use gpui::{
    Context, Div, FocusHandle, FontFeatures, MouseButton, Styled, Window, div, prelude::*, px, svg,
};
use std::time::{Duration, Instant};

/// Every control with its own hover and press motion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Close,
    Mute,
    Fullscreen,
    Subtitles,
    Audio,
    Sources,
    Settings,
    Play,
}

impl Control {
    pub const COUNT: usize = 8;
    /// The trigger of a panel.
    pub fn trigger(panel: PanelId) -> Self {
        match panel {
            PanelId::Subtitles => Self::Subtitles,
            PanelId::Audio => Self::Audio,
            PanelId::Sources => Self::Sources,
            PanelId::Settings => Self::Settings,
        }
    }
}

/// Hover fade and press scale of one control.
#[derive(Clone, Copy)]
pub struct ControlMotion {
    pub hover: Tween,
    pub press: Tween,
}

impl ControlMotion {
    pub fn new() -> Self {
        Self {
            hover: Tween::fixed(0.0),
            press: Tween::fixed(1.0),
        }
    }
    pub fn moving(self, now: Instant, reduced: bool) -> bool {
        self.hover.moving(now, reduced) || self.press.moving(now, reduced)
    }
}

/// Focus handles for every focusable part of the player.
pub struct Focuses {
    pub controls: [FocusHandle; Control::COUNT],
    pub volume: FocusHandle,
    pub seek: FocusHandle,
    pub panel: FocusHandle,
    pub retry: FocusHandle,
    pub back: FocusHandle,
    pub notice_sources: FocusHandle,
    pub notice_dismiss: FocusHandle,
    /// The value or colour field being edited.
    pub edit: FocusHandle,
    /// Stepper buttons, swatches, tabs and the settings controls, by `panel_widgets::slot`.
    pub pool: Vec<FocusHandle>,
}

impl Focuses {
    pub fn new(cx: &mut Context<Player>) -> Self {
        Self {
            controls: std::array::from_fn(|_| cx.focus_handle()),
            volume: cx.focus_handle(),
            seek: cx.focus_handle(),
            panel: cx.focus_handle(),
            retry: cx.focus_handle(),
            back: cx.focus_handle(),
            notice_sources: cx.focus_handle(),
            notice_dismiss: cx.focus_handle(),
            edit: cx.focus_handle(),
            pool: (0..super::panel_widgets::slot::COUNT)
                .map(|_| cx.focus_handle())
                .collect(),
        }
    }

    /// Whether a button (control, overlay or notice button) has focus.
    pub fn button_focused(&self, window: &Window) -> bool {
        self.controls
            .iter()
            .chain([
                &self.retry,
                &self.back,
                &self.notice_sources,
                &self.notice_dismiss,
            ])
            .any(|handle| handle.is_focused(window))
    }

    /// Whether keyboard focus rests on a chrome control, including the ranges and the panel.
    pub fn chrome_focused(&self, window: &Window) -> bool {
        self.controls
            .iter()
            .chain([&self.volume, &self.seek, &self.panel])
            .any(|handle| handle.is_focused(window))
    }
}

/// How a button sits on the video.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// Transparent inside a pill.
    None,
    /// A pill of its own: 72% canvas with a hairline ring.
    Pill,
    /// The active state of a standalone button: 20% ink with a ring.
    Active,
}

/// One icon button's look and state.
pub struct Button {
    pub control: Control,
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub size: f32,
    pub icon_size: f32,
    pub fill: Fill,
    /// Open or toggled on: a resting 15% ink fill.
    pub on: bool,
    pub disabled: bool,
}

/// `tabular-nums`.
pub fn tabular() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![
        ("tnum".into(), 1),
        ("kern".into(), 1),
    ]))
}

/// A hairline inset ring drawn above its parent's content.
pub fn ring(theme: Theme, alpha: f32) -> Div {
    div()
        .absolute()
        .inset_0()
        .rounded_full()
        .border_1()
        .border_color(theme.ink.opacity(alpha))
}

/// The shared pill surface: 72% canvas. Add `ring` as the last child so it paints above content.
pub fn pill(theme: Theme) -> Div {
    div()
        .relative()
        .rounded_full()
        .bg(theme.player_canvas.opacity(0.72))
}

pub fn focus_theme(theme: Theme) -> Theme {
    Theme {
        focus_inner: theme.player_canvas,
        focus_outer: theme.ink,
        ..theme
    }
}

const HOVER: Duration = Duration::from_millis(120);

impl Player {
    pub(super) fn retarget_control(
        &mut self,
        control: Control,
        hover: Option<bool>,
        press: Option<bool>,
    ) {
        let now = Instant::now();
        let motion = &mut self.motion[control as usize];
        if let Some(over) = hover {
            motion
                .hover
                .retarget(if over { 1.0 } else { 0.0 }, HOVER, Some(EASE_OUT), now);
        }
        if let Some(down) = press {
            motion
                .press
                .retarget(if down { 0.97 } else { 1.0 }, HOVER, Some(EASE_OUT), now);
        }
    }

    /// A 36 or 40 px round icon button with hover fade, press scale and the keyboard focus ring.
    pub(super) fn icon_button(
        &self,
        spec: Button,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Player, &mut Window, &mut Context<Player>) + 'static,
    ) -> Div {
        let now = Instant::now();
        let control = spec.control;
        let motion = self.motion[control as usize];
        let reduced = self.reduced;
        let hover = motion.hover.value(now, reduced);
        let scale = motion.press.value(now, reduced);
        let wash = match (spec.fill, spec.on) {
            (Fill::Active, _) => 0.2 - 0.1 * hover,
            (_, true) => 0.15 - 0.05 * hover,
            _ => 0.1 * hover,
        };
        let button = div()
            .id(spec.id)
            .role(gpui::Role::Button)
            .aria_label(spec.label)
            .size(px(spec.size * scale))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .when(!spec.disabled, |button| {
                button
                    .cursor_pointer()
                    .on_hover(cx.listener(move |player, over, _, cx| {
                        player.retarget_control(control, Some(*over), None);
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |player, _, _, cx| {
                            player.retarget_control(control, None, Some(true));
                            cx.notify();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |player, _, _, cx| {
                            player.retarget_control(control, None, Some(false));
                            cx.notify();
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(move |player, _, _, cx| {
                            player.retarget_control(control, None, Some(false));
                            cx.notify();
                        }),
                    )
                    .on_click(
                        cx.listener(move |player, _, window, cx| on_click(player, window, cx)),
                    )
            })
            .when(spec.disabled, |button| button.opacity(0.5))
            .when(spec.fill == Fill::Pill, |button| {
                button.bg(theme.player_canvas.opacity(0.72 * (1.0 - hover)))
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .bg(theme.ink.opacity(wash)),
            )
            .when(spec.fill != Fill::None, |button| {
                button.child(ring(theme, 0.12))
            })
            .child(
                svg()
                    .path(spec.icon)
                    .size(px(spec.icon_size * scale))
                    .text_color(theme.ink),
            );
        let focused = focus_ring(
            button,
            &self.focus.controls[control as usize],
            focus_theme(theme),
            !spec.disabled,
            keyboard,
            window,
        );
        div()
            .size(px(spec.size))
            .flex()
            .items_center()
            .justify_center()
            .child(focused)
    }
}
