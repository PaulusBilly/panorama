//! Controls of the subtitle and settings panels: stepper, swatches, segmented control,
//! checkbox and select trigger.
use super::{Player, measure::probe, prefs_ui::SelectId, widgets::focus_theme};
use crate::assets::Weighted;
use crate::{
    playback::subtitle_prefs::{FontWeight, Rgb},
    theme::{Theme, Typography, focus_ring},
};
use gpui::{Div, Stateful, Window, div, prelude::*, px, relative, rgb, svg};

/// Slots of `Focuses::pool`: three per stepper, then the other controls.
pub(super) mod slot {
    pub const STEPPERS: usize = 0;
    pub const SWATCHES: usize = 27;
    pub const CHOOSE: usize = 32;
    pub const WEIGHTS: usize = 33;
    pub const TABS: usize = 36;
    pub const SELECTS: usize = 39;
    pub const PASSTHROUGH: usize = 42;
    pub const DISCORD: usize = 43;
    pub const COUNT: usize = 48;
}

/// The text colour presets (`textColors`).
pub(super) const SWATCHES: [(&str, Rgb); 5] = [
    ("White", [0xff, 0xff, 0xff]),
    ("Yellow", [0xff, 0xe0, 0x66]),
    ("Green", [0xa3, 0xe6, 0xa1]),
    ("Blue", [0x8d, 0xe4, 0xff]),
    ("Pink", [0xff, 0xb5, 0xd5]),
];

pub(super) fn color(value: Rgb) -> gpui::Rgba {
    rgb(u32::from(value[0]) << 16 | u32::from(value[1]) << 8 | u32::from(value[2]))
}

/// A small caption line, `type-caption` in ink at `alpha`.
pub(super) fn note(text: impl Into<gpui::SharedString>, theme: Theme, alpha: f32) -> Div {
    div()
        .caption()
        .text_color(theme.ink.opacity(alpha))
        .child(text.into())
}

impl Player {
    /// The five preset colours and the colour wheel that opens the picker.
    pub(super) fn color_controls(
        &self,
        disabled: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Div {
        let ink = theme.ink;
        let current = self.ui.prefs.text_color;
        let cell = |id: gpui::ElementId, label: &'static str, selected: bool| {
            div()
                .id(id)
                .aria_label(label)
                .relative()
                .flex_1()
                .min_w_0()
                .min_h(px(44.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(ink.opacity(if selected { 0.2 } else { 0.1 }))
                .when(disabled, |cell| cell.opacity(0.4))
                .when(!disabled, |cell| {
                    cell.cursor_pointer().hover(move |style| {
                        style.bg(ink.opacity(if selected { 0.2 } else { 0.15 }))
                    })
                })
        };
        let swatches = SWATCHES.iter().enumerate().map(|(index, (label, value))| {
            let value = *value;
            let selected = current == value;
            let swatch = cell(("swatch", index).into(), label, selected)
                .role(gpui::Role::RadioButton)
                .when(!disabled, |cell| {
                    cell.on_click(cx.listener(move |player, _, _, cx| {
                        player.set_color(value, cx);
                    }))
                })
                .child(
                    div()
                        .size(px(24.0))
                        .rounded_full()
                        .bg(color(value))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(selected, |dot| {
                            dot.child(
                                svg()
                                    .path("check.svg")
                                    .size(px(16.0))
                                    .text_color(theme.player_canvas),
                            )
                        }),
                );
            focus_ring(
                swatch,
                &self.focus.pool[slot::SWATCHES + index],
                focus_theme(theme),
                !disabled,
                keyboard,
                0.0,
                window,
            )
        });
        // A colour off the presets is a custom one: the wheel holds it as a dot.
        let custom = !SWATCHES.iter().any(|(_, value)| *value == current);
        let wheel = cell(
            "choose-color".into(),
            "Custom color",
            custom || self.ui.color_open,
        )
        .role(gpui::Role::Button)
        .when(!disabled, |cell| {
            cell.on_click(cx.listener(|player, _, window, cx| {
                if player.ui.color_open {
                    player.cancel_edit(window, cx);
                } else {
                    player.open_color(window, cx);
                }
            }))
        })
        .child(probe(&self.measures, |measures, bounds| {
            measures.color_row.set(Some(bounds));
        }))
        .child(
            div()
                .relative()
                .size(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .child(gpui::img("color-wheel.svg").absolute().inset_0())
                .when(custom, |wheel| {
                    wheel.child(
                        div()
                            .size(px(12.0))
                            .rounded_full()
                            .border_2()
                            .border_color(theme.player_canvas)
                            .bg(color(current)),
                    )
                }),
        );
        let wheel = focus_ring(
            wheel,
            &self.focus.pool[slot::CHOOSE],
            focus_theme(theme),
            !disabled,
            keyboard,
            0.0,
            window,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(note("Text color", theme, 0.55))
            .child(div().flex().gap(px(4.0)).children(swatches).child(wheel))
    }

    /// The regular, medium, bold segmented control.
    pub(super) fn weight_controls(
        &self,
        disabled: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Div {
        let ink = theme.ink;
        let current = self.ui.prefs.font_weight;
        let items = FontWeight::ALL.iter().enumerate().map(|(index, weight)| {
            let weight = *weight;
            let selected = current == weight;
            let item = div()
                .id(("weight", index))
                .role(gpui::Role::RadioButton)
                .aria_label(weight.name())
                .flex_1()
                .min_h(px(36.0))
                .px(px(4.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.0))
                .weight(gpui::FontWeight(weight.css()))
                .when(selected, |item| item.bg(ink.opacity(0.15)))
                .when(!selected, |item| {
                    item.text_color(ink.opacity(0.7))
                        .hover(move |style| style.bg(ink.opacity(0.05)))
                })
                .when(disabled, |item| item.opacity(0.4))
                .when(!disabled, |item| {
                    item.cursor_pointer()
                        .on_click(cx.listener(move |player, _, _, cx| {
                            player.set_weight(weight, cx);
                        }))
                })
                .child(capitalised(weight.name()));
            focus_ring(
                item,
                &self.focus.pool[slot::WEIGHTS + index],
                focus_theme(theme),
                !disabled,
                keyboard,
                0.0,
                window,
            )
        });
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(note("Font weight", theme, 0.55))
            .child(
                div()
                    .flex()
                    .gap(px(2.0))
                    .bg(ink.opacity(0.1))
                    .p(px(2.0))
                    .children(items),
            )
    }

    /// A setting row with a switch at its end; the whole row toggles it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn checkbox(
        &self,
        id: &'static str,
        slot: usize,
        label: &'static str,
        checked: bool,
        disabled: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        on_toggle: impl Fn(&mut Player, &mut gpui::Context<Player>) + 'static,
        cx: &mut gpui::Context<Self>,
    ) -> Stateful<Div> {
        let ink = theme.ink;
        let row = setting_row(id, theme)
            .role(gpui::Role::CheckBox)
            .aria_label(label)
            .when(disabled, |row| row.opacity(0.5))
            .when(!disabled, |row| {
                row.cursor_pointer()
                    .hover(move |style| style.bg(ink.opacity(0.05)))
                    .on_click(cx.listener(move |player, _, _, cx| on_toggle(player, cx)))
            })
            .child(div().min_w_0().flex_1().child(label))
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(32.0))
                    .h(px(20.0))
                    .p(px(2.0))
                    .rounded_full()
                    .flex()
                    .when(checked, |track| track.justify_end().bg(theme.accent))
                    .when(!checked, |track| track.bg(ink.opacity(0.2)))
                    .child(div().size(px(16.0)).rounded_full().bg(ink)),
            );
        focus_ring(
            row,
            &self.focus.pool[slot],
            focus_theme(theme),
            !disabled,
            keyboard,
            0.0,
            window,
        )
    }

    /// A setting row that opens a select menu: the label, then the current value and a chevron.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn select_trigger(
        &self,
        select: SelectId,
        label: &'static str,
        value: String,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Stateful<Div> {
        let ink = theme.ink;
        let index = select.index();
        let open = self.ui.select.is_some_and(|open| open.id == select);
        let row = setting_row(("select", index), theme)
            .role(gpui::Role::ComboBox)
            .aria_label(select.label())
            .cursor_pointer()
            .when(open, |row| row.bg(ink.opacity(0.1)))
            .when(!open, |row| {
                row.hover(move |style| style.bg(ink.opacity(0.05)))
            })
            .on_click(cx.listener(move |player, _, _, cx| player.toggle_select(select, cx)))
            .child(probe(&self.measures, move |measures, bounds| {
                measures.selects[index].set(Some(bounds));
            }))
            .child(div().flex_shrink_0().child(label))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(4.0))
                    .text_color(ink.opacity(0.6))
                    .child(div().min_w_0().truncate().child(value))
                    .child(
                        svg()
                            .path("chevron-down.svg")
                            .size(px(14.0))
                            .flex_shrink_0()
                            .text_color(ink.opacity(0.6)),
                    ),
            );
        focus_ring(
            row,
            &self.focus.pool[slot::SELECTS + index],
            focus_theme(theme),
            true,
            keyboard,
            0.0,
            window,
        )
    }
}

/// The shared shape of a settings row, matching the option rows of the other panels.
fn setting_row(id: impl Into<gpui::ElementId>, theme: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .gap(px(12.0))
        .min_h(px(44.0))
        .px(px(20.0))
        .text_size(px(14.0))
        .line_height(relative(1.43))
        .text_color(theme.ink)
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swatches_are_the_tsx_presets() {
        let table: Vec<_> = SWATCHES
            .iter()
            .map(|(label, value)| (*label, crate::playback::subtitle_prefs::hex(*value)))
            .collect();
        assert_eq!(
            table,
            [
                ("White", "#ffffff".to_owned()),
                ("Yellow", "#ffe066".to_owned()),
                ("Green", "#a3e6a1".to_owned()),
                ("Blue", "#8de4ff".to_owned()),
                ("Pink", "#ffb5d5".to_owned()),
            ]
        );
        assert_eq!(capitalised("medium"), "Medium");
    }
}
