use crate::{
    app::AppShell,
    app_state::Account,
    sidebar::SIDEBAR_WIDTH,
    theme::{Typography, focus_ring, motion::*},
    transition::Tween,
};
use gpui::{
    Bounds, BoxShadow, Context, Div, FocusHandle, MouseDownEvent, Pixels, ScrollWheelEvent,
    Stateful, Window, div, point, prelude::*, px, rgba,
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

/// Which sidebar control the popup belongs to.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MenuKind {
    /// The avatar: the session.
    #[default]
    Account,
    /// The update item: the downloaded version.
    Update,
}

/// The popup's one row. The sidebar has its own items for Addons, the watch party and a ready
/// update, so the account popup holds the session alone.
pub fn entry(kind: MenuKind, signed_in: bool) -> MenuEntry {
    match kind {
        MenuKind::Update => MenuEntry::Update,
        MenuKind::Account if signed_in => MenuEntry::LogOut,
        MenuKind::Account => MenuEntry::LogIn,
    }
}

/// Retained account-popup presence and keyboard focus.
pub struct AccountMenu {
    /// Whether the popup accepts interaction.
    pub open: bool,
    /// The control it opened from; kept through the exit so the closing popup stays put.
    pub kind: MenuKind,
    presence: Tween,
    focus: FocusHandle,
}
impl AccountMenu {
    /// Allocate a focus target for each possible row.
    pub fn new(cx: &mut Context<AppShell>) -> Self {
        Self {
            open: false,
            kind: MenuKind::Account,
            presence: Tween::fixed(0.0),
            focus: cx.focus_handle().tab_stop(false),
        }
    }
    /// Whether the popup is open for `kind`.
    pub fn is_open(&self, kind: MenuKind) -> bool {
        self.open && self.kind == kind
    }
    /// Open for `kind`, or close when already open for it. Another control's open popup
    /// switches over without an exit.
    pub fn toggle(&mut self, kind: MenuKind) {
        if self.open && self.kind != kind {
            self.dismiss();
        }
        self.kind = kind;
        self.set_open(!self.open);
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
        self.focus.focus(window, cx);
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
        anchor: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<AppShell>,
    ) -> Stateful<Div> {
        let theme = view.theme;
        let reduced = view.reduced;
        let viewport = window.viewport_size();
        let progress = self.presence.value(Instant::now(), reduced);
        let rise = if reduced { 0.0 } else { 6.0 * (1.0 - progress) };
        let email = match account {
            Account::SignedIn(email) if self.kind == MenuKind::Account => Some(email.clone()),
            _ => None,
        };
        let entry = entry(self.kind, matches!(account, Account::SignedIn(_)));
        let label = entry.label();
        let caption = (entry == MenuEntry::Update)
            .then(|| update.map(|version| format!("Panorama {version} is ready")))
            .flatten();
        let mut shadow = popover_shadow();
        shadow[0].color = theme.rule.into();
        div()
            .id("account-menu")
            .role(gpui::Role::Menu)
            .aria_label(match self.kind {
                MenuKind::Account => "Account",
                MenuKind::Update => "Update",
            })
            .absolute()
            .left(px(SIDEBAR_WIDTH + 8.0))
            .bottom(viewport.height - anchor.bottom() - px(rise))
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
            .child(focus_ring(
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
                        shell.menu.set_open(false);
                        match entry {
                            MenuEntry::Update => shell.restart_to_update(cx),
                            MenuEntry::LogOut => {
                                shell.state.update(cx, |state, cx| state.sign_out(cx));
                                shell.sidebar.account.focus(window, cx);
                            }
                            MenuEntry::LogIn => shell.open_login(window, cx),
                        }
                        cx.notify();
                    }))
                    .child(div().label().child(label))
                    .when_some(caption, |row, caption| {
                        row.pb(px(2.0))
                            .child(div().caption().text_color(theme.ink_muted).child(caption))
                    }),
                &self.focus,
                theme,
                self.open,
                view.keyboard,
                0.0,
                window,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_popup_holds_the_session_and_the_update_popup_the_update() {
        assert_eq!(entry(MenuKind::Account, false), MenuEntry::LogIn);
        assert_eq!(entry(MenuKind::Account, true), MenuEntry::LogOut);
        assert_eq!(entry(MenuKind::Update, true), MenuEntry::Update);
        assert_eq!(entry(MenuKind::Update, false), MenuEntry::Update);
    }
}
