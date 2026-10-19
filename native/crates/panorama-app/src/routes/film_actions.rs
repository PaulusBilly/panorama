use super::*;
use crate::{
    film_display::Primary,
    theme::{focus_ring, motion::*},
};
use gpui::{Div, FontWeight, MouseButton, Transformation, div, prelude::*, px, relative, svg};

impl Film {
    pub(super) fn action_motion(&mut self, index: usize, pressed: bool, cx: &mut Context<Self>) {
        self.press[index].retarget(
            if pressed { 0.96 } else { 1.0 },
            DURATION_FAST,
            Some(EASE_DEFAULT),
            Instant::now(),
        );
        cx.notify();
    }
    pub(super) fn primary_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.account, Account::SignedIn(_)) {
            self.sign_in(window, cx);
            return;
        }
        if let Some(source) = self.selected.clone() {
            let id = self.id.clone();
            self.state.update(cx, |state, _| {
                state.playback = Some(crate::app_state::PlaybackSelection {
                    film_id: id.clone(),
                    source,
                })
            });
            let _ = self.shell.update(cx, |shell, cx| {
                shell.navigate(crate::router::Route::Player { id }, window, cx)
            });
        }
    }
    pub(super) fn actions(
        &mut self,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let primary = Primary::at(
            matches!(self.account, Account::SignedIn(_)),
            self.sources_loading,
            self.selected.is_some(),
        );
        let label = primary.label();
        let enabled = primary.enabled();
        let now = Instant::now();
        let press = self.press[0].value(now, view.reduced);
        let mut font = window.text_style().font();
        font.weight = FontWeight::MEDIUM;
        let text_width = f32::from(
            window
                .text_system()
                .shape_line(
                    label.into(),
                    px(17.0),
                    &[gpui::TextRun {
                        len: label.len(),
                        font,
                        ..Default::default()
                    }],
                    None,
                )
                .width,
        );
        let icon = if primary == Primary::Loading {
            Some("loader-2.svg")
        } else if primary == Primary::Play {
            Some("player-play-filled.svg")
        } else {
            None
        };
        let width = (60.0 + text_width + if icon.is_some() { 27.0 } else { 0.0 }).max(144.0);
        let button = focus_ring(
            div()
                .id("film-primary")
                .role(gpui::Role::Button)
                .aria_label(label)
                .w(px(width * press))
                .h(px(40.0 * press))
                .rounded_full()
                .bg(theme
                    .inverse
                    .opacity(1.0 - 0.1 * self.hover[0].value(now, view.reduced)))
                .text_color(theme.ink)
                .text_size(px(17.0 * press))
                .font_weight(FontWeight::MEDIUM)
                .line_height(relative(1.0))
                .pl(px(28.0 * press))
                .pr(px(32.0 * press))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(10.0 * press))
                .opacity(if enabled { 1.0 } else { 0.55 })
                .when(enabled, |button| button.cursor_pointer())
                .on_hover(cx.listener(|film, over, _, cx| {
                    film.hover[0].retarget(
                        if *over { 1.0 } else { 0.0 },
                        DURATION_FAST,
                        Some(EASE_DEFAULT),
                        Instant::now(),
                    );
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |film, _, _, cx| {
                        if enabled {
                            film.action_motion(0, true, cx);
                        }
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|film, _, _, cx| film.action_motion(0, false, cx)),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|film, _, _, cx| film.action_motion(0, false, cx)),
                )
                .on_click(cx.listener(move |film, _, window, cx| {
                    if enabled {
                        film.primary_action(window, cx);
                    }
                }))
                .when_some(icon, |button, icon| {
                    button.child(
                        svg()
                            .path(icon)
                            .size(px(17.0 * press))
                            .text_color(theme.ink)
                            .when(primary == Primary::Loading && !view.reduced, |icon| {
                                icon.with_transformation(Transformation::rotate(gpui::radians(
                                    now.duration_since(self.launched).as_secs_f32()
                                        * std::f32::consts::TAU,
                                )))
                            }),
                    )
                })
                .child(label),
            &self.primary,
            theme,
            view.active && enabled,
            view.keyboard,
            window,
        );
        let press = self.press[1].value(now, view.reduced);
        let progress = self.icon.value(now, view.reduced);
        let watchlist = focus_ring(
            div()
                .id("film-watchlist")
                .role(gpui::Role::Button)
                .aria_label(if self.saved {
                    "Remove from Watchlist"
                } else {
                    "Add to Watchlist"
                })
                .size(px(40.0 * press))
                .rounded_full()
                .bg(theme
                    .inverse
                    .opacity(0.15 + 0.1 * self.hover[1].value(now, view.reduced)))
                .cursor_pointer()
                .on_hover(cx.listener(|film, over, _, cx| {
                    film.hover[1].retarget(
                        if *over { 1.0 } else { 0.0 },
                        DURATION_FAST,
                        Some(EASE_DEFAULT),
                        Instant::now(),
                    );
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|film, _, _, cx| {
                        if !film.saving {
                            film.action_motion(1, true, cx);
                        }
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|film, _, _, cx| film.action_motion(1, false, cx)),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|film, _, _, cx| film.action_motion(1, false, cx)),
                )
                .on_click(cx.listener(|film, _, window, cx| film.toggle_watchlist(window, cx)))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .opacity(1.0 - progress)
                        .child(
                            svg()
                                .path(if self.previous_saved {
                                    "check.svg"
                                } else {
                                    "plus.svg"
                                })
                                .size(px(20.0 * (1.0 - 0.75 * progress) * press))
                                .text_color(theme.inverse),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .opacity(progress)
                        .child(
                            svg()
                                .path(if self.saved { "check.svg" } else { "plus.svg" })
                                .size(px(20.0 * (0.25 + 0.75 * progress) * press))
                                .text_color(theme.inverse),
                        ),
                ),
            &self.watchlist,
            theme,
            view.active && !self.saving,
            view.keyboard,
            window,
        );
        div()
            .mt(px(8.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .w(px(width))
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(button),
            )
            .child(
                div()
                    .size(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(watchlist),
            )
            .child(crate::a11y::status("film-primary-status", label.into()))
    }
    pub(super) fn retry(
        &self,
        details: bool,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let label = if details {
            "Retry details"
        } else {
            "Retry sources"
        };
        focus_ring(
            div()
                .id(label)
                .role(gpui::Role::Button)
                .aria_label(label)
                .min_h(px(40.0))
                .px(px(4.0))
                .border_b_1()
                .border_color(view.theme.inverse)
                .cursor_pointer()
                .flex()
                .items_center()
                .on_click(cx.listener(move |film, _, _, cx| {
                    if details {
                        film.load_details(cx);
                    } else {
                        film.load_sources(cx);
                    }
                }))
                .child(label),
            if details {
                &self.retry_details
            } else {
                &self.retry_sources
            },
            view.theme,
            view.active,
            view.keyboard,
            window,
        )
    }
}
