//! "Installed": rows in account order with move, configure and remove controls.
use super::{
    Addons, Slot,
    model::{InstalledRow, RowPending, Screen, error_copy},
    widgets::{View, logo, section, spinner, tnum},
};
use crate::{
    alert_row::AlertRow,
    button::{Button, Icon},
    film_card,
    home_layout::ViewSettings,
    theme::{SHIMMER, Typography},
};
use gpui::{Context, Div, KeyDownEvent, Window, div, prelude::*, px};
use std::time::Instant;

impl Addons {
    pub(super) fn installed_section(
        &mut self,
        current: Screen,
        view: View,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let count = self.rows.len();
        let trailing = (current == Screen::Ready)
            .then(|| format!("{count} addon{}", if count == 1 { "" } else { "s" }));
        let section = section(view, "Installed", trailing);
        match current {
            Screen::Loading => section.child(skeletons(view)),
            Screen::Failed => {
                let focus = self.handle(Slot::Retry, cx);
                section.child(
                    AlertRow {
                        message: "Couldn't load your addons. Check your connection and try again."
                            .into(),
                        label: "Try again",
                        theme,
                        width: view.width,
                        focus: &focus,
                        active: view.active,
                        keyboard: view.keyboard,
                    }
                    .render(cx.listener(|this, _, _, cx| this.reload(cx))),
                )
            }
            _ => {
                let now = Instant::now();
                let rows = self.rows.clone();
                let last = rows.len().saturating_sub(1);
                let mut list = div()
                    .id("addon-list")
                    .role(gpui::Role::List)
                    .aria_label("Installed addons")
                    .mt(px(16.0))
                    .border_t_1()
                    .border_color(theme.rule);
                for (index, row) in rows.iter().enumerate() {
                    list = list.child(self.render_row(index, last, row, now, view, window, cx));
                }
                section.child(list)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &mut self,
        index: usize,
        last: usize,
        row: &InstalledRow,
        now: Instant,
        view: View,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let theme = view.theme;
        let present = self
            .present
            .get(&row.key)
            .map_or(1.0, |t| t.value(now, view.reduced));
        let slide = self
            .slide
            .get(&row.key)
            .map_or(0.0, |t| t.value(now, view.reduced));
        let leaving = self.leaving.contains(&row.key);
        let image = self.logo_image(row.logo.as_ref(), 40.0, view, window, cx);
        let capabilities = row
            .capabilities
            .iter()
            .map(|capability| capability.label().to_uppercase())
            .collect::<Vec<_>>();
        let mut column = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.0))
                    .child(
                        div()
                            .heading()
                            .text_color(theme.ink)
                            .min_w_0()
                            .truncate()
                            .child(row.name.clone()),
                    )
                    .child(
                        div()
                            .caption()
                            .flex_shrink_0()
                            .font_features(tnum())
                            .text_color(theme.ink_muted)
                            .child(row.version.clone()),
                    ),
            )
            .child(
                div()
                    .mt(px(4.0))
                    .caption()
                    .text_color(theme.ink_muted)
                    .flex()
                    .gap(px(12.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .children(capabilities),
            );
        if let RowPending::Error(error) = row.pending {
            column = column.child(
                div()
                    .id(("row-error", index))
                    .mt(px(4.0))
                    .caption()
                    .text_color(theme.danger)
                    .role(gpui::Role::Alert)
                    .child(error_copy(&error)),
            );
        }
        let controls = self.controls(index, last, row, view, cx);
        div()
            .id(("addon-row", index))
            .relative()
            .top(px(slide))
            .opacity(present)
            .when(leaving, |outer| {
                outer.h(px(76.0 * present)).overflow_hidden()
            })
            .child(
                div()
                    .min_h(px(76.0))
                    .py(px(16.0))
                    .border_b_1()
                    .border_color(theme.rule)
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .child(logo(40.0, &row.name, image, false, theme))
                    .child(column)
                    .child(controls),
            )
    }

    fn controls(
        &mut self,
        index: usize,
        last: usize,
        row: &InstalledRow,
        view: View,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let key = row.key.clone();
        let name = row.name.clone();
        let strip = div().flex().items_center().gap(px(8.0)).flex_shrink_0();
        match row.pending {
            RowPending::ConfirmRemove => {
                let confirm = self.handle(Slot::ConfirmRemove(key.clone()), cx);
                let keep = self.handle(Slot::Keep(key.clone()), cx);
                let (remove_key, keep_key, escape_key) = (key.clone(), key.clone(), key.clone());
                strip
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            this.keep(&escape_key, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div()
                            .label()
                            .text_color(theme.ink)
                            .max_w(px(220.0))
                            .truncate()
                            .child(format!("Remove {name}?")),
                    )
                    .child(
                        Button::primary(("addon-confirm", index), "Remove")
                            .compact()
                            .aria_label(format!("Remove {name}"))
                            .focus(&confirm, view.keyboard)
                            .tab_stop(view.active)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove(remove_key.clone(), cx)
                            })),
                    )
                    .child(
                        Button::glass(("addon-keep", index), "Keep")
                            .compact()
                            .focus(&keep, view.keyboard)
                            .tab_stop(view.active)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.keep(&keep_key, window, cx)
                            })),
                    )
            }
            RowPending::Removing => strip
                .child(spinner(14.0, theme.ink_muted, view))
                .child(div().label().text_color(theme.ink_muted).child("Removing")),
            _ => {
                let up = self.handle(Slot::Up(key.clone()), cx);
                let down = self.handle(Slot::Down(key.clone()), cx);
                let (up_key, down_key) = (key.clone(), key.clone());
                let idle = !self.busy_rows();
                let mut strip =
                    strip
                        .child(
                            Button::round(
                                ("addon-up", index),
                                Icon::ArrowUp,
                                format!("Move {name} up"),
                            )
                            .compact()
                            .disabled(!(index > 0 && idle))
                            .focus(&up, view.keyboard)
                            .tab_stop(view.active)
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.move_by(up_key.clone(), -1, cx),
                            )),
                        )
                        .child(
                            Button::round(
                                ("addon-down", index),
                                Icon::ArrowDown,
                                format!("Move {name} down"),
                            )
                            .compact()
                            .disabled(!(index < last && idle))
                            .focus(&down, view.keyboard)
                            .tab_stop(view.active)
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.move_by(down_key.clone(), 1, cx),
                            )),
                        );
                if row.configurable {
                    let focus = self.handle(Slot::Configure(key.clone()), cx);
                    let configure_key = key.clone();
                    strip = strip.child(
                        Button::glass(("addon-configure", index), "Configure")
                            .compact()
                            .focus(&focus, view.keyboard)
                            .tab_stop(view.active)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.configure(configure_key.clone(), cx)
                            })),
                    );
                }
                if row.protected {
                    strip.child(
                        div()
                            .caption()
                            .text_color(theme.ink_muted)
                            .child("Built in"),
                    )
                } else {
                    let focus = self.handle(Slot::Remove(key.clone()), cx);
                    strip.child(
                        Button::glass(("addon-remove", index), "Remove")
                            .compact()
                            .aria_label(format!("Remove {name}"))
                            .focus(&focus, view.keyboard)
                            .tab_stop(view.active)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.ask_remove(&key, window, cx)
                            })),
                    )
                }
            }
        }
    }

    pub(super) fn ask_remove(
        &mut self,
        key: &panorama_core::addons::AddonKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_pending(key, RowPending::ConfirmRemove, cx);
        self.handle(Slot::Keep(key.clone()), cx).focus(window, cx);
    }

    pub(super) fn keep(
        &mut self,
        key: &panorama_core::addons::AddonKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_pending(key, RowPending::None, cx);
        self.handle(Slot::Remove(key.clone()), cx).focus(window, cx);
    }
}

fn skeletons(view: View) -> gpui::Stateful<Div> {
    let theme = view.theme;
    let settings = ViewSettings {
        width: view.width,
        height: 0.0,
        theme,
        active: view.active,
        keyboard: view.keyboard,
        reduced: view.reduced,
    };
    let phase = if view.reduced {
        0.0
    } else {
        (view.spin / std::f32::consts::TAU * 1000.0 / SHIMMER.duration.as_millis() as f32) % 1.0
    };
    let bar = |width: f32, height: f32| {
        div()
            .relative()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(theme.surface)
            .child(film_card::shimmer(settings, phase))
    };
    let mut list = div()
        .id("addon-skeleton")
        .mt(px(16.0))
        .border_t_1()
        .border_color(theme.rule)
        .role(gpui::Role::Status);
    for _ in 0..3 {
        list = list.child(
            div()
                .min_h(px(76.0))
                .py(px(16.0))
                .border_b_1()
                .border_color(theme.rule)
                .flex()
                .items_center()
                .gap(px(16.0))
                .child(bar(40.0, 40.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .child(bar(180.0, 16.0))
                        .child(bar(260.0, 10.0)),
                ),
        );
    }
    list
}
