use crate::{
    app::AppShell,
    router::Route,
    theme::{content_width, focus_ring, motion::*},
    transition::Tween,
};
use gpui::{BoxShadow, Context, Div, FocusHandle, Window, div, point, prelude::*, px, rgba};
use std::time::Instant;

/// One row of the account popup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuEntry {
    /// Opens the Addons screen.
    Addons,
    /// Ends the session.
    LogOut,
    /// Opens the sign-in dialog.
    LogIn,
}

impl MenuEntry {
    /// Visible and accessible label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Addons => "Addons",
            Self::LogOut => "Log Out",
            Self::LogIn => "Log In",
        }
    }
    /// The destination this entry navigates to, if it navigates.
    pub fn route(self) -> Option<Route> {
        (self == Self::Addons).then_some(Route::Addons)
    }
}

/// The popover border and two-layer drop shadow shared by the account and catalog menus.
pub fn popover_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            inset: false,
            color: rgba(0x0000000f).into(),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(1.0),
        },
        BoxShadow {
            inset: false,
            color: rgba(0x00000014).into(),
            offset: point(px(0.0), px(2.0)),
            blur_radius: px(6.0),
            spread_radius: px(0.0),
        },
        BoxShadow {
            inset: false,
            color: rgba(0x0000001a).into(),
            offset: point(px(0.0), px(10.0)),
            blur_radius: px(28.0),
            spread_radius: px(0.0),
        },
    ]
}

/// Entries in display order; signed out keeps only sign-in.
pub fn entries(signed_in: bool) -> &'static [MenuEntry] {
    if signed_in {
        &[MenuEntry::Addons, MenuEntry::LogOut]
    } else {
        &[MenuEntry::LogIn]
    }
}

/// Popup height: 12 px padding twice plus 40 px per entry.
pub fn height(signed_in: bool) -> f32 {
    24.0 + 40.0 * entries(signed_in).len() as f32
}

/// Retained account-popup presence and keyboard focus.
pub struct AccountMenu {
    /// Whether the popup accepts interaction.
    pub open: bool,
    presence: Tween,
    focus: Vec<FocusHandle>,
}
impl AccountMenu {
    /// Allocate the popup's single action.
    pub fn new(cx: &mut Context<AppShell>) -> Self {
        Self {
            open: false,
            presence: Tween::fixed(0.0),
            focus: (0..2).map(|_| cx.focus_handle().tab_stop(false)).collect(),
        }
    }
    /// Reverse popup presence from its current pose.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        self.presence.retarget(
            if open { 1.0 } else { 0.0 },
            if open { MENU_ENTER } else { MENU_EXIT },
            Some(if open { EASE_MENU } else { EASE_IN }),
            Instant::now(),
        );
    }
    /// Wheel, scroll and resize dismiss without an exit transition.
    pub fn dismiss(&mut self) {
        self.open = false;
        self.presence = Tween::fixed(0.0);
    }
    /// Whether presence needs more frames.
    pub fn moving(&self, reduced: bool) -> bool {
        self.presence.moving(Instant::now(), reduced)
    }
    /// Whether the popup remains painted for its exit.
    pub fn visible(&self, reduced: bool) -> bool {
        self.open || self.moving(reduced)
    }
    /// Render bottom-end anchoring with collision padding.
    pub fn render(
        &self,
        view: crate::home_layout::ViewSettings,
        signed_in: bool,
        window: &Window,
        cx: &mut Context<AppShell>,
    ) -> Div {
        let theme = view.theme;
        let keyboard = view.keyboard;
        let reduced = view.reduced;
        let width = f32::from(window.viewport_size().width);
        let progress = self.presence.value(Instant::now(), reduced);
        let scale = if reduced { 1.0 } else { 0.98 + 0.02 * progress };
        let offset = if reduced {
            0.0
        } else {
            -6.0 * (1.0 - progress)
        };
        let inset = ((width - content_width(width)) * 0.5).max(16.0);
        div()
            .absolute()
            .top(px(68.0 + offset))
            .right(px(inset))
            .w(px(192.0 * scale))
            .p(px(12.0 * scale))
            .bg(theme.canvas)
            .text_color(theme.ink)
            .opacity(progress)
            .occlude()
            .shadow(popover_shadow())
            .children(
                entries(signed_in)
                    .iter()
                    .zip(&self.focus)
                    .map(|(&entry, focus)| {
                        focus_ring(
                            div()
                                .id(("account-entry", entry as usize))
                                .w_full()
                                .min_h(px(40.0 * scale))
                                .px(px(4.0 * scale))
                                .border_b(px(scale))
                                .border_color(theme.ink)
                                .text_size(px(13.0 * scale))
                                .line_height(gpui::relative(1.4))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .role(gpui::Role::Button)
                                .aria_label(entry.label())
                                .on_click(cx.listener(move |shell, _, window, cx| {
                                    shell.menu.set_open(false);
                                    match entry.route() {
                                        Some(route) => shell.navigate(route, window, cx),
                                        None if entry == MenuEntry::LogOut => {
                                            shell.state.update(cx, |state, cx| state.sign_out(cx))
                                        }
                                        None => shell.open_login(window, cx),
                                    }
                                    cx.notify();
                                }))
                                .child(entry.label()),
                            focus,
                            theme,
                            self.open,
                            keyboard,
                            0.0,
                            window,
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_out_keeps_only_sign_in() {
        assert_eq!(entries(false), [MenuEntry::LogIn]);
        assert_eq!(entries(true), [MenuEntry::Addons, MenuEntry::LogOut]);
        assert_eq!(height(false), 64.0);
        assert_eq!(height(true), 104.0);
    }

    #[test]
    fn only_addons_navigates() {
        assert_eq!(MenuEntry::Addons.route(), Some(Route::Addons));
        assert_eq!(MenuEntry::LogOut.route(), None);
        assert_eq!(MenuEntry::LogIn.route(), None);
    }
}
