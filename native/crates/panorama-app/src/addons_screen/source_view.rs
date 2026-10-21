//! "Film details source": a radio group of Automatic plus every addon with movie metadata.
use super::{
    Addons, Slot,
    model::{error_copy, radio_step},
    widgets::{View, panel, section},
};
use crate::{
    theme::{Typography, focus_ring, motion::*},
    transition::Tween,
};
use gpui::{Context, Div, KeyDownEvent, Window, div, prelude::*, px};
use panorama_core::addons::AddonKey;
use std::time::Instant;

struct Choice {
    key: Option<AddonKey>,
    name: String,
    description: String,
}

impl Addons {
    fn choice_list(&self) -> Vec<Choice> {
        let mut list = vec![Choice {
            key: None,
            name: "Automatic".into(),
            description: "The first addon that answers".into(),
        }];
        list.extend(self.meta.choices.iter().map(|choice| {
            Choice {
                key: Some(choice.key.clone()),
                name: choice.name.clone(),
                description: self
                    .rows
                    .iter()
                    .find(|row| row.key == choice.key)
                    .map_or_else(String::new, |row| format!("Version {}", row.version)),
            }
        }));
        list
    }

    pub(super) fn source_section(
        &mut self,
        view: View,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let now = Instant::now();
        let choices = self.choice_list();
        let selected = choices
            .iter()
            .position(|choice| choice.key == self.meta.selected)
            .unwrap_or(0);
        let keys: Vec<Option<AddonKey>> = choices.iter().map(|choice| choice.key.clone()).collect();
        let last = choices.len().saturating_sub(1);
        let mut list = panel(theme)
            .id("meta-source")
            .role(gpui::Role::RadioGroup)
            .aria_label("Film details source");
        for (index, choice) in choices.into_iter().enumerate() {
            let dot = self
                .dots
                .entry(choice.key.clone())
                .or_insert_with(|| Tween::fixed(if index == selected { 1.0 } else { 0.0 }));
            dot.retarget(
                if index == selected { 1.0 } else { 0.0 },
                DURATION_FAST,
                Some(EASE_EDITORIAL),
                now,
            );
            let progress = dot.value(now, view.reduced);
            let focus = self.handle(Slot::Radio(choice.key.clone()), cx);
            let key = choice.key.clone();
            let steps = keys.clone();
            let row = focus_ring(
                div()
                    .id(("meta-choice", index))
                    .role(gpui::Role::RadioButton)
                    .aria_label(choice.name.clone())
                    .aria_toggled(if index == selected {
                        gpui::accesskit::Toggled::True
                    } else {
                        gpui::accesskit::Toggled::False
                    })
                    .min_h(px(52.0))
                    .when(index != last, |row| row.border_b_1().border_color(theme.rule))
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .cursor_pointer()
                    .on_click(cx.listener({
                        let key = key.clone();
                        move |this, _, _, cx| this.choose_meta(key.clone(), cx)
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        let delta = match event.keystroke.key.as_str() {
                            "up" | "left" => -1,
                            "down" | "right" => 1,
                            "space" | "enter" => {
                                this.choose_meta(key.clone(), cx);
                                cx.stop_propagation();
                                return;
                            }
                            _ => return,
                        };
                        let next = radio_step(steps.len(), index, delta);
                        if let Some(next) = steps.get(next).cloned() {
                            this.handle(Slot::Radio(next.clone()), cx).focus(window, cx);
                            this.choose_meta(next, cx);
                        }
                        cx.stop_propagation();
                    }))
                    .child(
                        div()
                            .size(px(16.0))
                            .flex_shrink_0()
                            .rounded_full()
                            .border_1()
                            .border_color(theme.ink)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(8.0 * (0.25 + 0.75 * progress)))
                                    .rounded_full()
                                    .bg(theme.ink)
                                    .opacity(progress),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .py(px(8.0))
                            .child(
                                div()
                                    .body()
                                    .text_color(theme.ink)
                                    .truncate()
                                    .child(choice.name),
                            )
                            .child(
                                div()
                                    .caption()
                                    .text_color(theme.ink_muted)
                                    .child(choice.description),
                            ),
                    ),
                &focus,
                theme,
                view.active && index == selected,
                view.keyboard,
                0.0,
                window,
            );
            list = list.child(row);
        }
        let mut section =
            section(view, "Film details source", None)
                .child(
                    div().mt(px(12.0)).body().text_color(theme.ink_muted).child(
                        "Which addon Panorama asks first for synopses, credits and artwork.",
                    ),
                )
                .child(list);
        if let Some(error) = &self.meta.error {
            section = section.child(
                div()
                    .id("meta-error")
                    .mt(px(8.0))
                    .caption()
                    .text_color(theme.danger)
                    .role(gpui::Role::Alert)
                    .child(error_copy(error)),
            );
        }
        section
    }
}
