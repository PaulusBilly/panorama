//! The Playback settings panel, the select menu and the custom colour popover.
use super::{
    Player,
    measure::probe,
    panel_widgets::{color, note, slot},
    prefs_ui::{SelectId, select_items},
    widgets::tabular,
};
use crate::{
    playback::{output, subtitle_prefs},
    theme::{Theme, Typography},
};
use gpui::{AnyElement, Bounds, Pixels, Window, div, prelude::*, px, relative, svg};

/// Sideways inset of a menu from the window edge, and the gap under its trigger.
const MENU_GAP: f32 = 4.0;
const COLLISION_PADDING: f32 = 20.0;
const ITEM_HEIGHT: f32 = 40.0;
/// Menu padding (`p-1`) and border on both sides.
const MENU_CHROME: f32 = 10.0;

/// Where a menu opens: below its trigger, or above when there is more room there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MenuPlacement {
    /// Open above the trigger.
    pub above: bool,
    /// The tallest the menu may be.
    pub max_height: f32,
}

/// Choose a side and height for a menu of `items` rows under a trigger at `top..bottom`
/// inside a root spanning `root_top..root_bottom`.
pub(super) fn place_menu(
    items: usize,
    top: f32,
    bottom: f32,
    root_top: f32,
    root_bottom: f32,
) -> MenuPlacement {
    let needed = items as f32 * ITEM_HEIGHT + MENU_CHROME;
    let below = (root_bottom - bottom - MENU_GAP - COLLISION_PADDING).max(ITEM_HEIGHT);
    let above = (top - root_top - MENU_GAP - COLLISION_PADDING).max(ITEM_HEIGHT);
    let flip = needed > below && above > below;
    MenuPlacement {
        above: flip,
        max_height: needed.min(if flip { above } else { below }),
    }
}

impl Player {
    fn device_label(&self) -> String {
        output::select_devices(&self.model.devices)
            .into_iter()
            .find(|device| device.name == self.ui.output.device)
            .map(|device| device.label)
            .unwrap_or_else(|| "System default".into())
    }

    /// The body under the title of the settings page.
    pub(super) fn settings_body(
        &self,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let devices = output::select_devices(&self.model.devices);
        let available = devices
            .iter()
            .find(|device| device.name == self.ui.output.device)
            .is_some_and(output::Device::passthrough_available);
        let passthrough = self.ui.output.passthrough() && available;
        let label_of = |id: SelectId| {
            let (items, current) = select_items(id, &self.ui.output, &self.model.devices);
            items
                .get(current)
                .map(|item| item.label.clone())
                .unwrap_or_default()
        };
        let helper = if available {
            "For a compatible receiver. Adjust volume on the receiver."
        } else {
            "Choose an output with confirmed receiver support to enable passthrough."
        };
        let status = output::status_line(self.ui.notice, passthrough);
        let mut body = div()
            .id("player-settings-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scrolls[3])
            .px(px(8.0))
            .pb(px(8.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.0))
                    .px(px(12.0))
                    .pb(px(8.0))
                    .child(self.select_trigger(
                        SelectId::Device,
                        self.device_label(),
                        false,
                        theme,
                        keyboard,
                        window,
                        cx,
                    ))
                    .child(self.select_trigger(
                        SelectId::Channels,
                        label_of(SelectId::Channels),
                        false,
                        theme,
                        keyboard,
                        window,
                        cx,
                    ))
                    .child(self.checkbox(
                        "settings-passthrough",
                        slot::PASSTHROUGH,
                        "Receiver passthrough",
                        self.ui.output.passthrough() && available,
                        !available,
                        theme,
                        keyboard,
                        window,
                        |player, cx| player.toggle_passthrough(cx),
                        cx,
                    ))
                    .child(note(helper, theme, 0.65))
                    .child(self.select_trigger(
                        SelectId::Video,
                        label_of(SelectId::Video),
                        false,
                        theme,
                        keyboard,
                        window,
                        cx,
                    ))
                    .child(
                        div()
                            .id("settings-status")
                            .role(gpui::Role::Status)
                            .child(note(status, theme, 0.65)),
                    ),
            );
        if let Some(discord) = &self.ui.discord {
            let enabled = discord.enabled;
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .px(px(12.0))
                    .pb(px(8.0))
                    .child(self.checkbox(
                        "settings-discord",
                        slot::DISCORD,
                        "Share watching activity on Discord",
                        enabled,
                        !discord.available || discord.busy,
                        theme,
                        keyboard,
                        window,
                        move |player, cx| player.set_discord(!enabled, cx),
                        cx,
                    ))
                    .child(note(discord.helper(), theme, 0.65))
                    .when_some(discord.error, |block, error| {
                        block.child(
                            div()
                                .id("discord-status")
                                .role(gpui::Role::Status)
                                .child(note(error, theme, 0.65)),
                        )
                    }),
            );
        }
        body.into_any_element()
    }

    fn popup_origin(&self, anchor: Bounds<Pixels>) -> (f32, f32) {
        let root = self.measures.root.get();
        (
            f32::from(anchor.left() - root.left()),
            f32::from(anchor.bottom() - root.top()),
        )
    }

    /// The open select menu or colour popover, positioned from the last frame's anchors.
    pub(super) fn popups(&self, theme: Theme, cx: &mut gpui::Context<Self>) -> Option<AnyElement> {
        self.measures.popup.set(None);
        if let Some(open) = self.ui.select {
            return self.select_menu(open.id, open.highlight, theme, cx);
        }
        if self.ui.color_open {
            return self.color_popover(theme);
        }
        None
    }

    fn select_menu(
        &self,
        id: SelectId,
        highlight: usize,
        theme: Theme,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let ink = theme.player_ink;
        let index = id.index();
        let trigger = self.measures.selects[index].get()?;
        let root = self.measures.root.get();
        let (items, current) = select_items(id, &self.ui.output, &self.model.devices);
        let placement = place_menu(
            items.len(),
            f32::from(trigger.top()),
            f32::from(trigger.bottom()),
            f32::from(root.top()),
            f32::from(root.bottom()),
        );
        let (left, bottom) = self.popup_origin(trigger);
        let top_edge = f32::from(trigger.top() - root.top());
        let rows = items.iter().enumerate().map(|(row, item)| {
            div()
                .id(("select-item", row))
                .role(gpui::Role::ListBoxOption)
                .aria_label(item.label.clone())
                .flex_shrink_0()
                .min_h(px(ITEM_HEIGHT))
                .px(px(12.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .text_size(px(14.0))
                .line_height(relative(1.43))
                .cursor_pointer()
                .when(highlight == row, |row| row.bg(ink.opacity(0.1)))
                .hover(move |style| style.bg(ink.opacity(0.1)))
                .on_click(cx.listener(move |player, _, _, cx| player.choose_select(id, row, cx)))
                .child(div().min_w_0().truncate().child(item.label.clone()))
                .when(row == current, |row| {
                    row.child(
                        svg()
                            .path("check.svg")
                            .size(px(16.0))
                            .flex_shrink_0()
                            .text_color(ink),
                    )
                })
        });
        let mut menu = div()
            .id("player-select-menu")
            .role(gpui::Role::ListBox)
            .absolute()
            .left(px(left))
            .min_w(trigger.size.width)
            .max_h(px(placement.max_height))
            .overflow_y_scroll()
            .rounded(px(8.0))
            .border_1()
            .border_color(ink.opacity(0.25))
            .bg(theme.player_canvas)
            .text_color(ink)
            .p(px(4.0))
            .shadow_lg()
            .occlude()
            .child(probe(&self.measures, |measures, bounds| {
                measures.popup.set(Some(bounds));
            }))
            .children(rows);
        menu = if placement.above {
            menu.bottom(px(f32::from(root.size.height) - top_edge + MENU_GAP))
        } else {
            menu.top(px(bottom + MENU_GAP))
        };
        Some(menu.into_any_element())
    }

    fn color_popover(&self, theme: Theme) -> Option<AnyElement> {
        let ink = theme.player_ink;
        let row = self.measures.color_row.get()?;
        let root = self.measures.root.get();
        let edit = self.ui.edit.as_ref()?;
        let (left, bottom) = self.popup_origin(row);
        let valid = subtitle_prefs::parse_strict_hex(&edit.draft);
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
        // Open above the row when the window ends too soon below it.
        let below = f32::from(root.bottom() - row.bottom()) > 150.0;
        let mut popover = div()
            .id("player-color-popover")
            .role(gpui::Role::Dialog)
            .aria_label("Custom text color")
            .absolute()
            .left(px(left))
            .w(row.size.width)
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(ink.opacity(0.25))
            .bg(theme.player_canvas)
            .text_color(ink)
            .shadow_lg()
            .occlude()
            .child(probe(&self.measures, |measures, bounds| {
                measures.popup.set(Some(bounds));
            }))
            .child(note("Custom color", theme, 0.55))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex_shrink_0()
                            .size(px(20.0))
                            .rounded_full()
                            .border_1()
                            .border_color(ink.opacity(0.25))
                            .bg(color(valid.unwrap_or(self.ui.prefs.text_color))),
                    )
                    .child(
                        div()
                            .id("hex-edit")
                            .role(gpui::Role::TextInput)
                            .aria_label("Custom text color")
                            .track_focus(&self.focus.edit)
                            .relative()
                            .flex_1()
                            .min_h(px(32.0))
                            .px(px(8.0))
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(ink.opacity(0.25))
                            .bg(theme.player_canvas)
                            .flex()
                            .items_center()
                            .text_size(px(14.0))
                            .font_features(tabular())
                            .child(probe(&self.measures, |measures, bounds| {
                                measures.edit.set(Some(bounds));
                            }))
                            .child(shown),
                    ),
            )
            .child(if valid.is_some() {
                note("Press Enter to close", theme, 0.55)
            } else {
                div()
                    .caption()
                    .text_color(theme.danger)
                    .child("Use the form #rrggbb")
            });
        popover = if below {
            popover.top(px(bottom + MENU_GAP))
        } else {
            popover.bottom(px(f32::from(root.size.height)
                - f32::from(row.top() - root.top())
                + MENU_GAP))
        };
        Some(popover.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_open_below_unless_there_is_more_room_above() {
        let below = place_menu(4, 300.0, 344.0, 32.0, 800.0);
        assert!(!below.above);
        assert_eq!(below.max_height, 170.0);
        let tight = place_menu(6, 600.0, 644.0, 32.0, 700.0);
        assert!(tight.above);
        assert_eq!(tight.max_height, 250.0);
        let long = place_menu(30, 300.0, 344.0, 32.0, 800.0);
        assert!(!long.above);
        assert_eq!(long.max_height, 800.0 - 344.0 - 4.0 - 20.0);
    }
}
