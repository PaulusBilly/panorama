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

const WIDTH: f32 = 224.0;

/// One row of the account popup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuEntry {
    /// Quits into the downloaded update and reopens.
    Update,
    /// Ends the session.
    LogOut,
    /// Opens the sign-in dialog.
    LogIn,
}

impl MenuEntry {
    /// Visible and accessible label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Update => "Restart to update",
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

/// Entries in display order: the sidebar already has Addons and the watch party, so a ready
/// update and the session remain.
pub fn entries(signed_in: bool, update_ready: bool) -> Vec<MenuEntry> {
    let session = if signed_in {
        MenuEntry::LogOut
    } else {
        MenuEntry::LogIn
    };
    update_ready
        .then_some(MenuEntry::Update)
        .into_iter()
        .chain([session])
        .collect()
}

/// Retained account-popup presence and keyboard focus.
pub struct AccountMenu {
    /// Whether the popup accepts interaction.
    pub open: bool,
    presence: Tween,
    focus: Vec<FocusHandle>,
}
impl AccountMenu {
    /// Allocate a focus target for each possible row.
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
    pub fn render(
        &self,
        view: crate::home_layout::ViewSettings,
        account: &Account,
        update: Option<&str>,
        window: &Window,
        cx: &mut Context<AppShell>,
    ) -> Stateful<Div> {
        let theme = view.theme;
        let ready = update.map(|version| format!("Panorama {version} is ready"));
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
            .children(
                entries(email_present(account), update.is_some())
                    .into_iter()
                    .zip(&self.focus)
                    .map(|(entry, focus)| {
                        let label = entry.label();
                        let caption = (entry == MenuEntry::Update)
                            .then(|| ready.clone())
                            .flatten();
                        focus_ring(
                            div()
                                .id(("account-entry", entry as usize))
                                .w_full()
                                .min_h(px(40.0))
                                .px(px(12.0))
                                .flex()
                                .flex_col()
                                .justify_center()
                                .cursor_pointer()
                                .hover(|style| style.bg(theme.hover))
                                .role(gpui::Role::MenuItem)
                                .aria_label(label)
                                .on_click(cx.listener(move |shell, _, window, cx| {
                                    match entry {
                                        MenuEntry::Update => {
                                            shell.menu.set_open(false);
                                            shell.restart_to_update(cx);
                                        }
                                        MenuEntry::LogOut => {
                                            shell.menu.set_open(false);
                                            shell.state.update(cx, |state, cx| state.sign_out(cx));
                                            shell.sidebar.account.focus(window, cx);
                                        }
                                        MenuEntry::LogIn => {
                                            shell.menu.set_open(false);
                                            shell.open_login(window, cx);
                                        }
                                    }
                                    cx.notify();
                                }))
                                .child(div().label().child(label))
                                .when_some(caption, |row, caption| {
                                    row.pb(px(2.0)).child(
                                        div().caption().text_color(theme.ink_muted).child(caption),
                                    )
                                }),
                            focus,
                            theme,
                            self.open,
                            view.keyboard,
                            0.0,
                            window,
                        )
                    }),
            )
    }
}

fn email_present(account: &Account) -> bool {
    matches!(account, Account::SignedIn(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_menu_has_no_party_entry() {
        assert_eq!(entries(false, false), [MenuEntry::LogIn]);
        assert_eq!(entries(true, false), [MenuEntry::LogOut]);
        for entry in entries(true, true).iter().chain(&entries(false, true)) {
            assert!(!entry.label().contains("party"), "{entry:?}");
        }
    }

    #[test]
    fn a_ready_update_leads_the_menu() {
        assert_eq!(entries(true, true), [MenuEntry::Update, MenuEntry::LogOut]);
        assert_eq!(entries(false, true), [MenuEntry::Update, MenuEntry::LogIn]);
    }
}
