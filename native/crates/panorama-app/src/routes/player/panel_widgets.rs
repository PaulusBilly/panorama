//! Controls of the subtitle and settings panels: stepper, swatches, segmented control,
//! checkbox and select trigger.
use super::{Player, measure::probe, prefs_ui::SelectId, widgets::focus_theme};
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
        .text_color(theme.player_ink.opacity(alpha))
        .child(text.into())
}

impl Player {
    /// The five preset colours and the "Choose color" row.
    pub(super) fn color_controls(
        &self,
        disabled: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Div {
        let ink = theme.player_ink;
        let current = self.ui.prefs.text_color;
        let swatches = SWATCHES.iter().enumerate().map(|(index, (label, value))| {
            let value = *value;
            let selected = current == value;
            let cell = div()
                .id(("swatch", index))
                .role(gpui::Role::RadioButton)
                .aria_label(*label)
                .flex_1()
                .min_w_0()
                .min_h(px(44.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(ink.opacity(if selected { 0.2 } else { 0.1 }))
                .when(disabled, |cell| cell.opacity(0.4))
                .when(!disabled, |cell| {
                    cell.cursor_pointer()
                        .on_click(cx.listener(move |player, _, _, cx| {
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
                cell,
                &self.focus.pool[slot::SWATCHES + index],
                focus_theme(theme),
                !disabled,
                keyboard,
                window,
            )
        });
        let choose = focus_ring(
            div()
                .id("choose-color")
                .role(gpui::Role::Button)
                .aria_label("Choose color")
                .relative()
                .min_h(px(44.0))
                .rounded_full()
                .bg(ink.opacity(0.1))
                .px(px(12.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .text_size(px(14.0))
                .line_height(relative(1.43))
                .when(disabled, |row| row.opacity(0.4))
                .when(!disabled, |row| {
                    row.cursor_pointer()
                        .on_click(cx.listener(|player, _, window, cx| {
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
                        .flex_shrink_0()
                        .size(px(20.0))
                        .rounded_full()
                        .border_1()
                        .border_color(ink.opacity(0.25))
                        .bg(color(current)),
                )
                .child("Choose color"),
            &self.focus.pool[slot::CHOOSE],
            focus_theme(theme),
            !disabled,
            keyboard,
            window,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(div().mb(px(0.0)).child(note("Text color", theme, 0.55)))
            .child(div().flex().gap(px(4.0)).children(swatches))
            .child(choose)
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
        let ink = theme.player_ink;
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
                .rounded(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight(weight.css()))
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
                    .gap(px(4.0))
                    .rounded(px(12.0))
                    .bg(ink.opacity(0.1))
                    .p(px(4.0))
                    .children(items),
            )
    }

    /// A checkbox with its label (`Checkbox.Root` inside a label).
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
        let ink = theme.player_ink;
        let row = div()
            .id(id)
            .role(gpui::Role::CheckBox)
            .aria_label(label)
            .flex()
            .items_center()
            .gap(px(12.0))
            .min_h(px(44.0))
            .text_size(px(14.0))
            .line_height(relative(1.43))
            .when(disabled, |row| row.opacity(0.5))
            .when(!disabled, |row| {
                row.cursor_pointer()
                    .on_click(cx.listener(move |player, _, _, cx| on_toggle(player, cx)))
            })
            .child(
                div()
                    .flex_shrink_0()
                    .size(px(16.0))
                    .rounded(px(2.0))
                    .border_1()
                    .border_color(ink.opacity(0.5))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(checked, |boxed| {
                        boxed.bg(ink).child(
                            svg()
                                .path("check.svg")
                                .size(px(13.0))
                                .text_color(theme.player_canvas),
                        )
                    }),
            )
            .child(div().min_w_0().child(label));
        focus_ring(
            row,
            &self.focus.pool[slot],
            focus_theme(theme),
            !disabled,
            keyboard,
            window,
        )
    }

    /// A select's field label and trigger button.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn select_trigger(
        &self,
        select: SelectId,
        value: String,
        disabled: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Div {
        let ink = theme.player_ink;
        let index = select.index();
        let trigger = div()
            .id(("select", index))
            .role(gpui::Role::ComboBox)
            .aria_label(select.label())
            .relative()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .w_full()
            .min_h(px(44.0))
            .px(px(12.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(ink.opacity(0.25))
            .bg(theme.player_canvas)
            .text_size(px(14.0))
            .line_height(relative(1.43))
            .when(disabled, |trigger| trigger.opacity(0.5))
            .when(!disabled, |trigger| {
                trigger
                    .cursor_pointer()
                    .on_click(cx.listener(move |player, _, _, cx| player.toggle_select(select, cx)))
            })
            .child(probe(&self.measures, move |measures, bounds| {
                measures.selects[index].set(Some(bounds));
            }))
            .child(div().min_w_0().truncate().child(value))
            .child(
                svg()
                    .path("chevron-down.svg")
                    .size(px(16.0))
                    .flex_shrink_0()
                    .text_color(ink),
            );
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .text_size(px(14.0))
            .line_height(relative(1.43))
            .child(select.label())
            .child(focus_ring(
                trigger,
                &self.focus.pool[slot::SELECTS + index],
                focus_theme(theme),
                !disabled,
                keyboard,
                window,
            ))
    }
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
