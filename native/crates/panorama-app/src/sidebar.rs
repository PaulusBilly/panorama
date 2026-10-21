//! The icon sidebar: drawn once by the shell over every route but the player.
pub use crate::theme::SIDEBAR_WIDTH;
use crate::{
    app::AppShell,
    app_state::Account,
    assets::Weighted,
    home_layout::ViewSettings,
    router::Destination,
    theme::{focus_ring, typography::LABEL},
    titlebar::SAFE_TOP,
};
use gpui::{
    Bounds, Context, Div, FocusHandle, FontWeight, Pixels, Size, Stateful, Window, bounds, div,
    point, prelude::*, px, size, svg,
};

/// Destination hit target; the hover circle.
const ITEM: f32 = 48.0;
/// Destination glyph.
const GLYPH: f32 = 24.0;
/// Width of the four-bar mark at the top.
const LOGO: f32 = 32.0;

/// Retained focus for the destinations and the account avatar.
pub struct Sidebar {
    items: [FocusHandle; Destination::ALL.len()],
    /// The avatar, refocused when the account menu or sign-in closes.
    pub(crate) account: FocusHandle,
}

impl Sidebar {
    /// Allocate focus once for the shell's lifetime.
    pub fn new(cx: &mut Context<AppShell>) -> Self {
        Self {
            items: std::array::from_fn(|_| cx.focus_handle()),
            account: cx.focus_handle(),
        }
    }

    /// Focus the item for `active`, or the first item when no destination is current.
    pub fn focus(
        &self,
        active: Option<Destination>,
        window: &mut Window,
        cx: &mut Context<AppShell>,
    ) {
        let index = active
            .and_then(|active| Destination::ALL.iter().position(|d| *d == active))
            .unwrap_or(0);
        self.items[index].focus(window, cx);
    }

    /// No fill on any route; routes that draw media start their left scrim at x = 0.
    /// Logo at top = SAFE_TOP + 14; items centred; avatar at `avatar_bounds`.
    /// Active: ink + 3px accent bar at x = 0; inactive: ink_muted; hover: glass circle.
    pub fn render(
        &self,
        active: Option<Destination>,
        account: &Account,
        menu_open: bool,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<AppShell>,
    ) -> Stateful<Div> {
        let theme = view.theme;
        let items = Destination::ALL
            .iter()
            .zip(&self.items)
            .map(|(&destination, focus)| {
                let current = active == Some(destination);
                div()
                    .relative()
                    .w(px(SIDEBAR_WIDTH))
                    .h(px(ITEM))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(current, |item| {
                        item.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px((ITEM - GLYPH) * 0.5))
                                .w(px(3.0))
                                .h(px(GLYPH))
                                .rounded_r(px(1.5))
                                .bg(theme.accent),
                        )
                    })
                    .child(focus_ring(
                        div()
                            .id(("sidebar-item", destination as usize))
                            .size(px(ITEM))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .role(gpui::Role::Link)
                            .aria_label(destination.label())
                            .when(current, |item| item.aria_description("Current page"))
                            .when(destination == Destination::Search, |item| {
                                item.aria_keyshortcuts("/")
                            })
                            .hover(|style| style.bg(theme.glass))
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.go(destination, window, cx)
                            }))
                            .child(
                                svg()
                                    .path(destination.icon().path())
                                    .size(px(GLYPH))
                                    .text_color(if current { theme.ink } else { theme.ink_muted }),
                            ),
                        focus,
                        theme,
                        view.active,
                        view.keyboard,
                        ITEM * 0.5,
                        window,
                    ))
            });
        let avatar = avatar_bounds(window.viewport_size());
        let initial = match account {
            Account::SignedIn(email) => email.chars().next().map(|c| c.to_uppercase().to_string()),
            _ => None,
        };
        let diameter = f32::from(avatar.size.width);
        div()
            .id("sidebar")
            .role(gpui::Role::Navigation)
            .aria_label("Panorama")
            .absolute()
            .top_0()
            .left_0()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .text_color(theme.ink)
            .child(
                svg()
                    .path("icon.svg")
                    .absolute()
                    .top(px(SAFE_TOP + 14.0))
                    .left(px((SIDEBAR_WIDTH - LOGO) * 0.5))
                    .w(px(LOGO))
                    .h(px(LOGO * 197.0 / 366.0))
                    .text_color(theme.ink),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(8.0))
                    .children(items),
            )
            .child(focus_ring(
                div()
                    .id("sidebar-account")
                    .absolute()
                    .left(avatar.origin.x)
                    .top(avatar.origin.y)
                    .size(px(diameter))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .bg(if menu_open {
                        theme.glass_hover
                    } else {
                        theme.glass
                    })
                    .hover(|style| style.bg(theme.glass_hover))
                    .role(gpui::Role::Button)
                    .aria_label("Account menu")
                    .aria_expanded(menu_open)
                    .on_click(cx.listener(|shell, event: &gpui::ClickEvent, window, cx| {
                        shell.menu.set_open(!shell.menu.open);
                        // Opened from the keyboard, focus moves into the menu.
                        if shell.menu.open && matches!(event, gpui::ClickEvent::Keyboard(_)) {
                            shell.menu.focus_first(window, cx);
                        }
                        cx.notify();
                    }))
                    .map(|avatar| match initial {
                        Some(initial) => {
                            avatar.child(LABEL.apply(div()).weight(FontWeight::BOLD).child(initial))
                        }
                        None => avatar.child(
                            svg()
                                .path(crate::button::Icon::Person.path())
                                .size(px(20.0))
                                .text_color(theme.ink),
                        ),
                    }),
                &self.account,
                theme,
                view.active,
                view.keyboard,
                diameter * 0.5,
                window,
            ))
    }
}

/// The one source of the avatar geometry. The sidebar paints the avatar here and the account
/// menu anchors to it.
pub fn avatar_bounds(viewport: Size<Pixels>) -> Bounds<Pixels> {
    let diameter = 40.0;
    bounds(
        point(
            px((SIDEBAR_WIDTH - diameter) * 0.5),
            viewport.height - px(24.0 + diameter),
        ),
        size(px(diameter), px(diameter)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn avatar_sits_in_the_bottom_of_the_sidebar() {
        let avatar = avatar_bounds(size(px(1280.0), px(800.0)));
        assert_eq!(avatar.origin, point(px(20.0), px(736.0)));
        assert_eq!(avatar.size, size(px(40.0), px(40.0)));
        assert_eq!(avatar.bottom(), px(776.0));
    }
}
