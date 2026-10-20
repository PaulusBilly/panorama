use super::*;
use crate::{
    account_menu::popover_shadow,
    addons_screen::model::{CatalogGroup, NAME_CAP, display_text},
    catalog_switcher::{BLOCK_HEIGHT, current, flatten, offers_choice, step},
    theme::{Typography, focus_ring},
};
use gpui::{Div, KeyDownEvent, Transformation, deferred, div, svg};
use panorama_core::addons::CatalogRef;

const TRIGGER_HEIGHT: f32 = 44.0;

impl Home {
    pub(crate) fn switcher_visible(&self, cx: &Context<Self>) -> bool {
        self.search.is_none() && offers_choice(self.state.read(cx).switcher_groups())
    }

    /// Space between the hero and the first card row.
    pub(crate) fn lead(&self, cx: &Context<Self>) -> f32 {
        if self.switcher_visible(cx) {
            BLOCK_HEIGHT
        } else {
            32.0
        }
    }

    fn current_catalog(&self, flat: &[CatalogRef], cx: &Context<Self>) -> Option<CatalogRef> {
        let state = self.state.read(cx);
        current(
            flat,
            self.switcher.pending.as_ref(),
            state.remembered.as_ref(),
            state.catalog.as_ref().and_then(|page| page.source.as_ref()),
        )
    }

    fn pick(&mut self, catalog: CatalogRef, window: &mut Window, cx: &mut Context<Self>) {
        self.switcher.choose(catalog);
        self.switcher.trigger.focus(window, cx);
        cx.notify();
    }

    pub(crate) fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let groups = self.state.read(cx).switcher_groups().to_vec();
        let flat = flatten(&groups);
        let current = self.current_catalog(&flat, cx);
        self.switcher.toggle();
        if self.switcher.open {
            self.switcher.active = current
                .and_then(|current| {
                    flat.iter().position(|catalog| {
                        catalog.addon == current.addon && catalog.catalog_id == current.catalog_id
                    })
                })
                .unwrap_or(0);
            self.switcher.menu.focus(window, cx);
        }
        cx.notify();
    }

    fn close_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.switcher.set_open(false);
        self.switcher.trigger.focus(window, cx);
        cx.notify();
    }

    pub(super) fn catalog_row(
        &mut self,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let groups = self.state.read(cx).switcher_groups().to_vec();
        let flat = flatten(&groups);
        let current = self.current_catalog(&flat, cx);
        let is_current = |catalog: &CatalogRef| {
            current.as_ref().is_some_and(|current| {
                current.addon == catalog.addon && current.catalog_id == catalog.catalog_id
            })
        };
        let (name, addon) = current.as_ref().map_or_else(
            || (String::new(), String::new()),
            |current| {
                (
                    display_text(&current.name, NAME_CAP),
                    groups
                        .iter()
                        .find(|group| group.catalogs.iter().any(|c| c.addon == current.addon))
                        .map_or_else(String::new, |group| group.addon.clone()),
                )
            },
        );
        let progress = self.switcher.presence.value(Instant::now(), view.reduced);
        let trigger = focus_ring(
            div()
                .id("catalog-trigger")
                .group("catalog-trigger")
                .h(px(TRIGGER_HEIGHT))
                .max_w(px(480.0))
                .min_w_0()
                .flex()
                .flex_col()
                .items_start()
                .cursor_pointer()
                .role(gpui::Role::Button)
                .aria_label(format!("Catalog: {name}, {addon}"))
                .aria_expanded(self.switcher.open)
                .on_click(cx.listener(|home, _, window, cx| home.open_switcher(window, cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .max_w_full()
                        .min_w_0()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .title()
                                .text_color(theme.ink)
                                .border_b_1()
                                .border_color(theme.canvas.opacity(0.0))
                                .group_hover("catalog-trigger", |style| {
                                    style.border_color(theme.ink)
                                })
                                .child(name),
                        )
                        .child(
                            svg()
                                .path("chevron-down-thin.svg")
                                .size(px(20.0))
                                .text_color(theme.ink)
                                .when(!view.reduced, |icon| {
                                    icon.with_transformation(Transformation::rotate(gpui::radians(
                                        std::f32::consts::PI * progress,
                                    )))
                                }),
                        ),
                )
                .child(
                    div()
                        .mt(px(2.0))
                        .caption()
                        .max_w_full()
                        .truncate()
                        .text_color(theme.ink_muted)
                        .child(addon.to_uppercase()),
                ),
            &self.switcher.trigger,
            theme,
            view.active,
            view.keyboard,
            0.0,
            window,
        )
        .key_context("PanoramaCard")
        .on_action(cx.listener(|home, _: &NextCard, window, cx| home.focus_first_card(window, cx)))
        .on_action(cx.listener(|home, _: &PreviousCard, window, cx| {
            home.watch.focus(window, cx);
            cx.stop_propagation();
        }));
        let mut row = div()
            .relative()
            .h(px(BLOCK_HEIGHT))
            .pt(px(48.0))
            .pb(px(24.0))
            .flex()
            .child(trigger);
        if self.switcher.visible(view.reduced) {
            row = row.child(
                deferred(self.catalog_menu(&groups, &is_current, progress, view, cx))
                    .with_priority(1),
            );
        }
        row
    }

    fn catalog_menu(
        &self,
        groups: &[CatalogGroup],
        is_current: &dyn Fn(&CatalogRef) -> bool,
        progress: f32,
        view: ViewSettings,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let theme = view.theme;
        let scale = if view.reduced {
            1.0
        } else {
            0.98 + 0.02 * progress
        };
        let offset = if view.reduced {
            0.0
        } else {
            -6.0 * (1.0 - progress)
        };
        let flat = flatten(groups);
        let count = flat.len();
        let mut panel = div()
            .id("catalog-menu")
            .role(gpui::Role::Menu)
            .aria_label("Catalogs")
            .absolute()
            .top(px(48.0 + TRIGGER_HEIGHT + 8.0 + offset))
            .left_0()
            .w(px(320.0 * scale))
            .max_h(px(420.0))
            .overflow_y_scroll()
            .bg(theme.canvas)
            .text_color(theme.ink)
            .opacity(progress)
            .occlude()
            .shadow(popover_shadow())
            .track_focus(&self.switcher.menu)
            .on_mouse_down_out(cx.listener(|home, _, _, cx| {
                home.switcher.set_open(false);
                cx.notify();
            }))
            .on_key_down(cx.listener(move |home, event: &KeyDownEvent, window, cx| {
                let flat = flatten(home.state.read(cx).switcher_groups());
                match event.keystroke.key.as_str() {
                    "escape" => home.close_switcher(window, cx),
                    "up" => home.switcher.active = step(flat.len(), home.switcher.active, -1),
                    "down" => home.switcher.active = step(flat.len(), home.switcher.active, 1),
                    "enter" | "space" => {
                        if let Some(catalog) = flat.get(home.switcher.active).cloned() {
                            home.pick(catalog, window, cx);
                        }
                    }
                    _ => return,
                }
                cx.notify();
                cx.stop_propagation();
            }));
        let mut index = 0;
        for group in groups {
            panel = panel.child(
                div()
                    .caption()
                    .text_color(theme.ink_muted)
                    .px(px(16.0))
                    .pt(px(12.0))
                    .pb(px(4.0))
                    .truncate()
                    .child(group.addon.to_uppercase()),
            );
            for catalog in &group.catalogs {
                let chosen = is_current(catalog);
                let active = self.switcher.active == index && index < count;
                let target = catalog.clone();
                let label = display_text(&catalog.name, NAME_CAP);
                panel = panel.child(
                    div()
                        .id(("catalog-option", index))
                        .role(gpui::Role::MenuItem)
                        .aria_label(label.clone())
                        .min_h(px(40.0))
                        .px(px(16.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .label()
                        .cursor_pointer()
                        .bg(if chosen {
                            theme.selected
                        } else if active {
                            theme.hover
                        } else {
                            theme.canvas.opacity(0.0)
                        })
                        .hover(|style| style.bg(if chosen { theme.selected } else { theme.hover }))
                        .on_click(cx.listener(move |home, _, window, cx| {
                            home.pick(target.clone(), window, cx)
                        }))
                        .child(div().min_w_0().truncate().child(label))
                        .when(chosen, |row| {
                            row.child(
                                svg()
                                    .path("check.svg")
                                    .size(px(16.0))
                                    .flex_shrink_0()
                                    .text_color(theme.ink),
                            )
                        }),
                );
                index += 1;
            }
        }
        panel
    }

    /// Reveal the trigger row, then focus it.
    pub(crate) fn focus_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = self.settings(window, cx);
        self.scroll
            .set_offset(point(px(0.0), px(-self.content_start(view, cx))));
        self.switcher.trigger.focus(window, cx);
        self.tick(window, cx);
        cx.stop_propagation();
        cx.notify();
    }
}

impl Home {
    /// Commit a chosen catalog once the cards have faded out, then fade the new ones in.
    pub(super) fn advance_swap(&mut self, reduced: bool, cx: &mut Context<Self>) {
        let now = Instant::now();
        if let Some(catalog) = self.switcher.take_ready(now, reduced) {
            self.state
                .update(cx, |state, cx| state.select_catalog(catalog, cx));
            return;
        }
        if self.switcher.awaiting {
            let state = self.state.read(cx);
            if state.catalog.is_some() {
                self.switcher.arrived(true);
            } else if !state.loading {
                self.switcher.arrived(false);
            }
        }
    }
}
