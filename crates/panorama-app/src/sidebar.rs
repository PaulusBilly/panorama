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

const ITEM: f32 = 48.0;
const GLYPH: f32 = 24.0;
const LOGO: f32 = 32.0;
const LOGO_TOP: f32 = SAFE_TOP + 14.0;
const LOGO_HEIGHT: f32 = LOGO * 197.0 / 366.0;
/// Vertical centre of the wordmark, so top-bar controls on routes can line up with it.
pub const LOGO_CENTER: f32 = LOGO_TOP + LOGO_HEIGHT * 0.5;

pub struct Sidebar {
    items: [FocusHandle; Destination::ALL.len()],
    pub(crate) join: FocusHandle,
    pub(crate) account: FocusHandle,
}

/// The sidebar's watch party action, declared beside the avatar it sits above.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartyEntry {
    /// Opens the field for an invite link.
    Join,
    /// In a party: opens its panel.
    Watch,
}

impl PartyEntry {
    /// Joining plays the host's film, which needs an account, so signed out there is none.
    pub fn for_session(signed_in: bool, in_party: bool) -> Option<Self> {
        match (signed_in, in_party) {
            (_, true) => Some(Self::Watch),
            (true, false) => Some(Self::Join),
            (false, false) => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Join => "Join watch party",
            Self::Watch => "Watch party",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Join => "link.svg",
            Self::Watch => crate::button::Icon::Users.path(),
        }
    }
}

/// What the sidebar's bottom controls show: which popover is open, and whether a party is on.
#[derive(Clone, Copy, Debug, Default)]
pub struct SidebarStatus {
    pub account_menu: bool,
    pub join: bool,
    pub in_party: bool,
    /// A downloaded update waits in the account menu.
    pub update_ready: bool,
}

impl Sidebar {
    pub fn new(cx: &mut Context<AppShell>) -> Self {
        Self {
            items: std::array::from_fn(|_| cx.focus_handle()),
            join: cx.focus_handle(),
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
        status: SidebarStatus,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<AppShell>,
    ) -> Stateful<Div> {
        let theme = view.theme;
        let party =
            PartyEntry::for_session(matches!(account, Account::SignedIn(_)), status.in_party)
                .map(|entry| self.party_item(entry, status.join, view, window, cx));
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
        let menu_open = status.account_menu;
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
                    .top(px(LOGO_TOP))
                    .left(px((SIDEBAR_WIDTH - LOGO) * 0.5))
                    .w(px(LOGO))
                    .h(px(LOGO_HEIGHT))
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
            .children(party)
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
                    .when(status.update_ready, |avatar| {
                        avatar.aria_description("Update ready")
                    })
                    .aria_expanded(menu_open)
                    .on_click(cx.listener(|shell, event: &gpui::ClickEvent, window, cx| {
                        shell.menu.set_open(!shell.menu.open);
                        if shell.menu.open && matches!(event, gpui::ClickEvent::Keyboard(_)) {
                            shell.menu.focus_first(window, cx);
                        }
                        cx.notify();
                    }))
                    .when(status.update_ready, |avatar| {
                        avatar.child(
                            div()
                                .absolute()
                                .top(px(0.0))
                                .right(px(0.0))
                                .size(px(10.0))
                                .rounded_full()
                                .border_2()
                                .border_color(theme.canvas)
                                .bg(theme.accent),
                        )
                    })
                    .map(|avatar| match initial {
                        Some(initial) => avatar
                            .child(LABEL.apply(div()).weight(FontWeight::MEDIUM).child(initial)),
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

impl Sidebar {
    /// Join watch party, or in a party, Watch party with a live dot, just above the avatar.
    fn party_item(
        &self,
        entry: PartyEntry,
        open: bool,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<AppShell>,
    ) -> Div {
        let theme = view.theme;
        let at = join_bounds(window.viewport_size());
        let watching = entry == PartyEntry::Watch;
        div()
            .absolute()
            .left(at.origin.x)
            .top(at.origin.y)
            .child(focus_ring(
                div()
                    .id("sidebar-party")
                    .relative()
                    .size(px(ITEM))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .when(open, |item| item.bg(theme.glass))
                    .hover(|style| style.bg(theme.glass))
                    .role(gpui::Role::Button)
                    .aria_label(entry.label())
                    .when(!watching, |item| item.aria_expanded(open))
                    .on_click(cx.listener(move |shell, _, window, cx| match entry {
                        PartyEntry::Join => shell.toggle_join(window, cx),
                        PartyEntry::Watch => shell.open_party_panel(window, cx),
                    }))
                    .child(svg().path(entry.icon()).size(px(GLYPH)).text_color(
                        if open || watching {
                            theme.ink
                        } else {
                            theme.ink_muted
                        },
                    ))
                    .when(watching, |item| {
                        item.child(
                            div()
                                .absolute()
                                .top(px(10.0))
                                .right(px(9.0))
                                .size(px(8.0))
                                .rounded_full()
                                .border_1()
                                .border_color(theme.canvas)
                                .bg(theme.accent),
                        )
                    }),
                &self.join,
                theme,
                view.active,
                view.keyboard,
                ITEM * 0.5,
                window,
            ))
    }
}

/// Gap between the party item and the avatar under it.
const PARTY_GAP: f32 = 8.0;

/// Where the party item sits: one item's height above the avatar, centred in the sidebar.
/// The join popover anchors to it.
pub fn join_bounds(viewport: Size<Pixels>) -> Bounds<Pixels> {
    let avatar = avatar_bounds(viewport);
    bounds(
        point(
            px((SIDEBAR_WIDTH - ITEM) * 0.5),
            avatar.origin.y - px(PARTY_GAP + ITEM),
        ),
        size(px(ITEM), px(ITEM)),
    )
}

/// Gap between the avatar and the window's bottom edge.
pub const AVATAR_INSET: f32 = 24.0;

/// The one source of the avatar geometry. The sidebar paints the avatar here and the account
/// menu anchors to it.
pub fn avatar_bounds(viewport: Size<Pixels>) -> Bounds<Pixels> {
    let diameter = 40.0;
    bounds(
        point(
            px((SIDEBAR_WIDTH - diameter) * 0.5),
            viewport.height - px(AVATAR_INSET + diameter),
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

    #[test]
    fn the_party_item_joins_signed_in_and_reads_watch_party_in_a_party() {
        assert_eq!(PartyEntry::for_session(false, false), None);
        assert_eq!(PartyEntry::for_session(true, false), Some(PartyEntry::Join));
        assert_eq!(PartyEntry::for_session(true, true), Some(PartyEntry::Watch));
        assert_eq!(PartyEntry::Join.label(), "Join watch party");
        assert_eq!(PartyEntry::Watch.label(), "Watch party");
    }

    #[test]
    fn the_party_item_sits_just_above_the_avatar() {
        let viewport = size(px(1280.0), px(800.0));
        let item = join_bounds(viewport);
        assert_eq!(
            item.bottom() + px(PARTY_GAP),
            avatar_bounds(viewport).origin.y
        );
        assert_eq!(item.center().x, avatar_bounds(viewport).center().x);
    }
}
