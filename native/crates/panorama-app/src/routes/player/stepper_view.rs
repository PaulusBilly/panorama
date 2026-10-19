//! The stepper control (`PlayerStepperControl`): label, minus, editable value, plus.
use super::{
    Player,
    edit::Target,
    measure::probe,
    panel_widgets::{note, slot},
    widgets::{focus_theme, tabular},
};
use crate::{
    playback::subtitle_prefs::Field,
    theme::{Theme, focus_ring},
};
use gpui::{AnyElement, Div, Stateful, Window, div, prelude::*, px, svg};

impl Player {
    /// A 44 px round stepper button with the Tabler icon.
    #[allow(clippy::too_many_arguments)]
    fn stepper_button(
        &self,
        field: Field,
        direction: i32,
        slot: usize,
        disabled: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Stateful<Div> {
        let ink = theme.player_ink;
        let button = div()
            .id(("stepper", slot))
            .role(gpui::Role::Button)
            .aria_label(field.action_label(direction > 0))
            .flex_shrink_0()
            .size(px(44.0))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .when(disabled, |button| button.opacity(0.4))
            .when(!disabled, |button| {
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(ink.opacity(0.1)))
                    .on_click(cx.listener(move |player, _, window, cx| {
                        player.commit_edit(window, cx);
                        player.adjust(field, direction, cx);
                    }))
            })
            .child(
                svg()
                    .path(if direction > 0 {
                        "plus.svg"
                    } else {
                        "minus.svg"
                    })
                    .size(px(18.0))
                    .text_color(ink),
            );
        focus_ring(
            button,
            &self.focus.pool[slot],
            focus_theme(theme),
            !disabled,
            keyboard,
            window,
        )
    }

    /// `PlayerStepperControl`: label, then minus, an editable value and plus in a pill.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn stepper(
        &self,
        index: usize,
        field: Field,
        disabled: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Div {
        let ink = theme.player_ink;
        let value = self.ui.prefs.get(field);
        let base = slot::STEPPERS + index * 3;
        let editing = self
            .ui
            .edit
            .as_ref()
            .filter(|edit| edit.target == Target::Field(field));
        let middle: AnyElement = if let Some(edit) = editing {
            let text = div().child(edit.draft.clone());
            let shown = if edit.fresh {
                text.bg(ink.opacity(0.25)).into_any_element()
            } else {
                div()
                    .flex()
                    .items_center()
                    .child(text)
                    .child(div().w(px(1.0)).h(px(16.0)).bg(ink))
                    .into_any_element()
            };
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .font_features(tabular())
                .child(
                    div()
                        .id(("stepper-edit", index))
                        .role(gpui::Role::TextInput)
                        .aria_label(format!("Edit {}", field.label().to_lowercase()))
                        .track_focus(&self.focus.edit)
                        .relative()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(64.0))
                        .min_h(px(32.0))
                        .px(px(8.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(ink.opacity(0.25))
                        .bg(theme.player_canvas)
                        .text_color(ink)
                        .child(probe(&self.measures, |measures, bounds| {
                            measures.edit.set(Some(bounds));
                        }))
                        .child(shown),
                )
                .child(div().child(field.suffix()))
                .into_any_element()
        } else if field.editable() {
            div()
                .id(("stepper-value", index))
                .role(gpui::Role::Button)
                .aria_label(format!("Edit {}", field.label().to_lowercase()))
                .flex_1()
                .min_w_0()
                .min_h(px(44.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .font_features(tabular())
                .when(disabled, |value| value.opacity(0.4))
                .when(!disabled, |value| {
                    value
                        .cursor_pointer()
                        .on_click(cx.listener(move |player, _, window, cx| {
                            player.begin_edit(field, window, cx);
                        }))
                })
                .child(field.text(value))
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .justify_center()
                .text_size(px(14.0))
                .font_features(tabular())
                .when(disabled, |value| value.opacity(0.4))
                .child(field.text(value))
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(note(field.label(), theme, 0.55))
            .child(
                div()
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(ink.opacity(0.1))
                    .child(
                        self.stepper_button(field, -1, base, disabled, theme, keyboard, window, cx),
                    )
                    .child(middle)
                    .child(self.stepper_button(
                        field,
                        1,
                        base + 2,
                        disabled,
                        theme,
                        keyboard,
                        window,
                        cx,
                    )),
            )
    }
}
