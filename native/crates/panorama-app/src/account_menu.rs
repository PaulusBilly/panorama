use crate::{
    app::AppShell,
    app_state::Account,
    sidebar::{SIDEBAR_WIDTH, avatar_bounds},
    theme::{Typography, focus_ring, motion::*},
    transition::Tween,
};
use gpui::{
    BoxShadow, Context, Div, FocusHandle, MouseDownEvent, ScrollWheelEvent, Stateful, Window, div,
    point, prelude::*, px, rgba,
};
use std::time::Instant;

/// Popup width.
const WIDTH: f32 = 224.0;

/// One row of the account popup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuEntry {
    /// Ends the session.
    LogOut,
    /// Opens the sign-in dialog.
    LogIn,
}

impl MenuEntry {
    /// Visible and accessible label.
    pub fn label(self) -> &'static str {
        match self {
            Self::LogOut => "Sign out",
            Self::LogIn => "Sign in",
        }
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

/// Entries in display order: the sidebar already has Addons, so only the session remains.
pub fn entries(signed_in: bool) -> &'static [MenuEntry] {
    if signed_in {
        &[MenuEntry::LogOut]
    } else {
        &[MenuEntry::LogIn]
    }
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
            focus: vec![cx.focus_handle().tab_stop(false)],
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
    /// Move keyboard focus to the first entry, for menus opened from the keyboard.
    pub fn focus_first(&self, window: &mut Window, cx: &mut Context<AppShell>) {
        if let Some(first) = self.focus.first() {
            first.focus(window, cx);
        }
    }
    /// While open, a full-window transparent layer under the popup: any press or wheel outside
    /// the popup lands here and closes it without reaching the page.
    pub fn catcher(&self, cx: &mut Context<AppShell>) -> Stateful<Div> {
        div()
            .id("account-menu-catcher")
            .absolute()
            .inset_0()
            .occlude()
            .on_any_mouse_down(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
                shell.keyboard_navigation = false;
                shell.menu.set_open(false);
                cx.notify();
            }))
            .on_scroll_wheel(cx.listener(|shell, _: &ScrollWheelEvent, _, cx| {
                shell.menu.dismiss();
                cx.notify();
            }))
    }
    /// Render beside the avatar: `left = SIDEBAR_WIDTH + 8`, bottom aligned with the avatar.
    pub fn render(
        &self,
        view: crate::home_layout::ViewSettings,
        account: &Account,
        window: &Window,
        cx: &mut Context<AppShell>,
    ) -> Stateful<Div> {
        let theme = view.theme;
        let reduced = view.reduced;
        let viewport = window.viewport_size();
        let avatar = avatar_bounds(viewport);
        let progress = self.presence.value(Instant::now(), reduced);
        let rise = if reduced { 0.0 } else { 6.0 * (1.0 - progress) };
        let email = match account {
            Account::SignedIn(email) => Some(email.clone()),
            _ => None,
        };
        let mut shadow = popover_shadow();
        shadow[0].color = theme.rule.into();
        div()
            .id("account-menu")
            .role(gpui::Role::Menu)
            .aria_label("Account")
            .absolute()
            .left(px(SIDEBAR_WIDTH + 8.0))
            .bottom(viewport.height - avatar.bottom() - px(rise))
            .w(px(WIDTH))
            .p(px(4.0))
            .bg(theme.surface)
            .text_color(theme.ink)
            .opacity(progress)
            .occlude()
            .shadow(shadow)
            .when_some(email, |menu, email| {
                menu.child(
                    div()
                        .px(px(12.0))
                        .pt(px(10.0))
                        .pb(px(12.0))
                        .mb(px(4.0))
                        .border_b_1()
                        .border_color(theme.rule)
                        .child(
                            div()
                                .caption()
                                .text_color(theme.ink_muted)
                                .child("Signed in as"),
                        )
                        .child(div().label().truncate().child(email)),
                )
            })
            .children(entries(email_present(account)).iter().zip(&self.focus).map(
                |(&entry, focus)| {
                    focus_ring(
                        div()
                            .id(("account-entry", entry as usize))
                            .w_full()
                            .min_h(px(40.0))
                            .px(px(12.0))
                            .label()
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(theme.hover))
                            .role(gpui::Role::MenuItem)
                            .aria_label(entry.label())
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.menu.set_open(false);
                                if entry == MenuEntry::LogOut {
                                    shell.state.update(cx, |state, cx| state.sign_out(cx));
                                    shell.sidebar.account.focus(window, cx);
                                } else {
                                    shell.open_login(window, cx);
                                }
                                cx.notify();
                            }))
                            .child(entry.label()),
                        focus,
                        theme,
                        self.open,
                        view.keyboard,
                        0.0,
                        window,
                    )
                },
            ))
    }
}

fn email_present(account: &Account) -> bool {
    matches!(account, Account::SignedIn(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_session_entry_per_account_state() {
        assert_eq!(entries(false), [MenuEntry::LogIn]);
        assert_eq!(entries(true), [MenuEntry::LogOut]);
    }
}
