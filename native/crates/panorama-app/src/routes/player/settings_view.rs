//! The Playback settings panel, the select menu and the custom colour popover.
use super::{
    Player,
    measure::probe,
    panel_widgets::{color, note, slot},
    picker::{Hsv, Part},
    prefs_ui::{SelectId, select_items},
    widgets::tabular,
};
use crate::{
    playback::{output, subtitle_prefs},
    theme::{Theme, Typography},
};
use gpui::{
    AnyElement, Bounds, Div, MouseButton, MouseDownEvent, Pixels, Stateful, Window, div,
    linear_color_stop, linear_gradient, prelude::*, px, relative, svg,
};

/// Sideways inset of a menu from the window edge, and the gap under its trigger.
const MENU_GAP: f32 = 4.0;
const COLLISION_PADDING: f32 = 20.0;
const ITEM_HEIGHT: f32 = 40.0;
/// Menu padding (`p-1`) and border on both sides.
const MENU_CHROME: f32 = 10.0;
/// About the colour popover's height; below that much room under its row it opens above.
const POPOVER_HEIGHT: f32 = 330.0;
const POPOVER_WIDTH: f32 = 272.0;
/// The picker's drag handles.
const THUMB: f32 = 16.0;

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
            "Sends the original audio to your receiver. Set the volume there."
        } else {
            "Needs an output with a confirmed receiver."
        };
        let section = |label: &'static str| {
            note(label, theme, 0.55)
                .px(px(20.0))
                .pt(px(12.0))
                .pb(px(4.0))
        };
        let helper_line = |text: &'static str| note(text, theme, 0.55).px(px(20.0)).pb(px(8.0));
        let mut body = div()
            .id("player-settings-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scrolls[3])
            .flex()
            .flex_col()
            .px(px(0.0))
            .pb(px(8.0))
            .child(section("Audio"))
            .child(self.select_trigger(
                SelectId::Device,
                "Output",
                self.device_label(),
                theme,
                keyboard,
                window,
                cx,
            ))
            .child(self.select_trigger(
                SelectId::Channels,
                "Channels",
                label_of(SelectId::Channels),
                theme,
                keyboard,
                window,
                cx,
            ))
            .child(self.checkbox(
                "settings-passthrough",
                slot::PASSTHROUGH,
                "Receiver passthrough",
                passthrough,
                !available,
                theme,
                keyboard,
                window,
                |player, cx| player.toggle_passthrough(cx),
                cx,
            ))
            .child(helper_line(helper))
            .when_some(self.ui.notice, |body, notice| {
                body.child(
                    div()
                        .id("settings-status")
                        .role(gpui::Role::Status)
                        .child(helper_line(notice)),
                )
            })
            .child(section("Video"))
            .child(self.select_trigger(
                SelectId::Video,
                "Output",
                label_of(SelectId::Video),
                theme,
                keyboard,
                window,
                cx,
            ));
        if let Some(discord) = &self.ui.discord {
            let enabled = discord.enabled;
            body = body
                .child(section("Sharing"))
                .child(self.checkbox(
                    "settings-discord",
                    slot::DISCORD,
                    "Discord activity",
                    enabled,
                    !discord.available || discord.busy,
                    theme,
                    keyboard,
                    window,
                    move |player, cx| player.set_discord(!enabled, cx),
                    cx,
                ))
                .child(helper_line(discord.helper()))
                .when_some(discord.error, |body, error| {
                    body.child(
                        div()
                            .id("discord-status")
                            .role(gpui::Role::Status)
                            .child(helper_line(error)),
                    )
                });
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
            return self.color_popover(theme, cx);
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
        let ink = theme.ink;
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

    fn color_popover(&self, theme: Theme, cx: &mut gpui::Context<Self>) -> Option<AnyElement> {
        let ink = theme.ink;
        let row = self.measures.color_row.get()?;
        let root = self.measures.root.get();
        let edit = self.ui.edit.as_ref()?;
        let (left, bottom) = self.popup_origin(row);
        // Right-aligned under the wheel, kept inside the window.
        let left = (left + f32::from(row.size.width) - POPOVER_WIDTH).max(MENU_GAP);
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
        let hsv = self.ui.picker;
        // Open above the row when the window ends too soon below it.
        let below = f32::from(root.bottom() - row.bottom()) > POPOVER_HEIGHT;
        let mut popover = div()
            .id("player-color-popover")
            .role(gpui::Role::Dialog)
            .aria_label("Custom text color")
            .absolute()
            .left(px(left))
            .w(px(POPOVER_WIDTH))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(12.0))
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
            .child(self.picker_field(hsv, cx))
            .child(self.picker_hue(hsv, cx))
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

impl Player {
    /// A drag handle: a white ring around the colour it sits on.
    fn picker_thumb(fill: gpui::Rgba) -> Div {
        div()
            .absolute()
            .size(px(THUMB))
            .ml(px(-THUMB / 2.0))
            .rounded_full()
            .border_2()
            .border_color(gpui::white())
            .bg(fill)
            .shadow_md()
    }

    fn picker_press_handler(
        part: Part,
        cx: &mut gpui::Context<Self>,
    ) -> impl Fn(&MouseDownEvent, &mut Window, &mut gpui::App) + 'static {
        cx.listener(move |player, event: &MouseDownEvent, _, cx| {
            player.picker_press(part, event.position, cx);
        })
    }

    /// Saturation runs left to right over the hue, brightness falls top to bottom.
    fn picker_field(&self, hsv: Hsv, cx: &mut gpui::Context<Self>) -> Stateful<Div> {
        let layer = |fill: gpui::Background| div().absolute().inset_0().bg(fill);
        let white = gpui::white();
        let black = gpui::black();
        div()
            .id("color-field")
            .role(gpui::Role::Slider)
            .aria_label("Saturation and brightness")
            .relative()
            .w_full()
            .h(px(152.0))
            .bg(color(Hsv::hue_rgb(hsv.h)))
            .cursor_crosshair()
            .on_mouse_down(
                MouseButton::Left,
                Self::picker_press_handler(Part::Field, cx),
            )
            .child(layer(linear_gradient(
                90.0,
                linear_color_stop(white, 0.0),
                linear_color_stop(white.opacity(0.0), 1.0),
            )))
            .child(layer(linear_gradient(
                180.0,
                linear_color_stop(black.opacity(0.0), 0.0),
                linear_color_stop(black, 1.0),
            )))
            .child(probe(&self.measures, |measures, bounds| {
                measures.picker[Part::Field as usize].set(Some(bounds));
            }))
            .child(
                Self::picker_thumb(color(hsv.to_rgb()))
                    .left(relative(hsv.s))
                    .top(relative(1.0 - hsv.v))
                    .mt(px(-THUMB / 2.0)),
            )
    }

    /// The hue strip: six two-stop gradients through the colour wheel.
    fn picker_hue(&self, hsv: Hsv, cx: &mut gpui::Context<Self>) -> Stateful<Div> {
        let segments = (0..6).map(|index| {
            let stop = |at: usize| color(Hsv::hue_rgb(at as f32 / 6.0));
            div().flex_1().h_full().bg(linear_gradient(
                90.0,
                linear_color_stop(stop(index), 0.0),
                linear_color_stop(stop(index + 1), 1.0),
            ))
        });
        div()
            .id("color-hue")
            .role(gpui::Role::Slider)
            .aria_label("Hue")
            .relative()
            .w_full()
            .h(px(THUMB))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, Self::picker_press_handler(Part::Hue, cx))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(2.0))
                    .h(px(12.0))
                    .flex()
                    .children(segments),
            )
            .child(probe(&self.measures, |measures, bounds| {
                measures.picker[Part::Hue as usize].set(Some(bounds));
            }))
            .child(
                Self::picker_thumb(color(Hsv::hue_rgb(hsv.h)))
                    .left(relative(hsv.h))
                    .top_0(),
            )
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
