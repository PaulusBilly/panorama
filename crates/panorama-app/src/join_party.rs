//! The sidebar's way into a watch party: a field for the invite link the host sent.
use crate::{
    account_menu::popover_shadow,
    app::AppShell,
    assets::Weighted,
    button::Button,
    home_layout::ViewSettings,
    sidebar::{SIDEBAR_WIDTH, join_bounds},
    theme::{Typography, motion::*},
    transition::Tween,
};
use gpui::{
    Context, Div, Entity, FocusHandle, Focusable, FontWeight, MouseDownEvent, ScrollWheelEvent,
    Stateful, Subscription, Window, div, prelude::*, px, relative,
};
use gpui_component::input::{Input, InputEvent, InputState};
use panorama_core::party::transport::{InviteError, parse_invite};
use std::time::Instant;

const WIDTH: f32 = 320.0;

/// Why a pasted link did not join.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Problem {
    Empty,
    NotInvite,
    Broken,
}

impl Problem {
    pub fn of(invite: &str, error: InviteError) -> Self {
        if invite.trim().is_empty() {
            Self::Empty
        } else if matches!(error, InviteError::Scheme) {
            Self::NotInvite
        } else {
            Self::Broken
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Self::Empty => {
                "Paste the invite link first. The host can copy it from their party panel."
            }
            Self::NotInvite => "That isn't a Panorama invite link.",
            Self::Broken => "That invite link is incomplete. Ask the host to copy it again.",
        }
    }
}

/// The Join watch party popover beside the sidebar.
pub struct JoinParty {
    pub open: bool,
    presence: Tween,
    input: Option<Entity<InputState>>,
    problem: Option<Problem>,
    submit: FocusHandle,
    leave: FocusHandle,
    _events: Option<Subscription>,
}

impl JoinParty {
    pub fn new(cx: &mut Context<AppShell>) -> Self {
        Self {
            open: false,
            presence: Tween::fixed(0.0),
            input: None,
            problem: None,
            submit: cx.focus_handle(),
            leave: cx.focus_handle(),
            _events: None,
        }
    }

    fn set_open(&mut self, open: bool) {
        self.open = open;
        self.problem = None;
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

    pub fn moving(&self, reduced: bool) -> bool {
        self.presence.moving(Instant::now(), reduced)
    }

    pub fn visible(&self, reduced: bool) -> bool {
        self.open || self.moving(reduced)
    }
}

impl AppShell {
    /// Opens the field, filled in when the clipboard already holds an invite link, or closes it.
    pub(crate) fn toggle_join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.join.open {
            self.close_join(window, cx);
            return;
        }
        self.menu.set_open(false);
        self.join.set_open(true);
        let input = match &self.join.input {
            Some(input) => input.clone(),
            None => {
                let input =
                    cx.new(|cx| InputState::new(window, cx).placeholder("Paste the invite link"));
                self.join._events = Some(cx.subscribe_in(
                    &input,
                    window,
                    |shell, _, event, window, cx| match event {
                        InputEvent::PressEnter { .. } => shell.submit_join(window, cx),
                        InputEvent::Change if shell.join.problem.is_some() => {
                            shell.join.problem = None;
                            cx.notify();
                        }
                        _ => {}
                    },
                ));
                self.join.input = Some(input.clone());
                input
            }
        };
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .filter(|text| parse_invite(text).is_ok())
            .unwrap_or_default();
        input.update(cx, |input, cx| input.set_value(copied.trim(), window, cx));
        input.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub(crate) fn close_join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.join.set_open(false);
        self.sidebar.join.focus(window, cx);
        cx.notify();
    }

    fn submit_join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(invite) = self
            .join
            .input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
        else {
            return;
        };
        if let Err(error) = self.join_party_from_invite(&invite, None, window, cx) {
            self.join.problem = Some(Problem::of(&invite, error));
            if let Some(input) = &self.join.input {
                input.focus_handle(cx).focus(window, cx);
            }
        }
        cx.notify();
    }

    fn leave_party_from_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.leave_party(cx));
        self.close_join(window, cx);
    }

    /// While open, a full-window transparent layer under the popover: a press or wheel outside
    /// it closes it without reaching the page.
    pub(crate) fn join_catcher(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("join-party-catcher")
            .absolute()
            .inset_0()
            .occlude()
            .on_any_mouse_down(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
                shell.keyboard_navigation = false;
                shell.join.set_open(false);
                cx.notify();
            }))
            .on_scroll_wheel(cx.listener(|shell, _: &ScrollWheelEvent, _, cx| {
                shell.join.dismiss();
                cx.notify();
            }))
    }

    /// The popover beside the sidebar's party item: the invite field, or, in a party that has
    /// not opened a film yet, a way out of it.
    pub(crate) fn join_popover(
        &self,
        view: ViewSettings,
        in_party: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = view.theme;
        let viewport = window.viewport_size();
        let anchor = join_bounds(viewport);
        let progress = self.join.presence.value(Instant::now(), view.reduced);
        let rise = if view.reduced {
            0.0
        } else {
            6.0 * (1.0 - progress)
        };
        let mut shadow = popover_shadow();
        shadow[0].color = theme.rule.into();
        let (title, detail) = if in_party {
            (
                "Watch party",
                "You're in a watch party. It opens here when the host starts the film.",
            )
        } else {
            (
                "Join a watch party",
                "Paste the invite link the host sent you.",
            )
        };
        let popover = div()
            .id("join-party")
            .role(gpui::Role::Dialog)
            .aria_label(title)
            .absolute()
            .left(px(SIDEBAR_WIDTH + 8.0))
            .bottom(viewport.height - anchor.bottom() - px(rise))
            .w(px(WIDTH))
            .p(px(16.0))
            .flex()
            .flex_col()
            .bg(theme.surface)
            .text_color(theme.ink)
            .opacity(progress)
            .occlude()
            .shadow(shadow)
            .child(div().label().weight(FontWeight::MEDIUM).child(title))
            .child(
                div()
                    .mt(px(4.0))
                    .caption()
                    .text_color(theme.ink_muted)
                    .child(detail),
            );
        if in_party {
            return popover.child(
                div().mt(px(16.0)).flex().justify_end().child(
                    Button::glass("join-party-leave", "Leave party")
                        .compact()
                        .focus(&self.join.leave, view.keyboard)
                        .tab_stop(self.join.open)
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.leave_party_from_sidebar(window, cx)
                        })),
                ),
            );
        }
        let Some(input) = self.join.input.clone() else {
            return popover;
        };
        let focused = input.focus_handle(cx).is_focused(window);
        popover
            .child(
                div()
                    .mt(px(16.0))
                    .pb(px(if focused { 7.0 } else { 8.0 }))
                    .border_b(px(if focused { 2.0 } else { 1.0 }))
                    .border_color(if self.join.problem.is_some() {
                        theme.danger
                    } else {
                        theme.ink
                    })
                    .child(
                        Input::new(&input)
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .aria_label("Invite link")
                            .map(|input| gpui::Styled::h(input, px(24.0)))
                            .p_0()
                            .text_size(px(14.0))
                            .line_height(relative(1.5))
                            .text_color(theme.ink),
                    ),
            )
            .when_some(self.join.problem, |popover, problem| {
                popover.child(
                    div()
                        .id("join-party-problem")
                        .mt(px(8.0))
                        .caption()
                        .text_color(theme.danger)
                        .role(gpui::Role::Alert)
                        .child(problem.text()),
                )
            })
            .child(
                div().mt(px(16.0)).flex().justify_end().child(
                    Button::primary("join-party-submit", "Join")
                        .aria_label("Join watch party")
                        .compact()
                        .focus(&self.join.submit, view.keyboard)
                        .tab_stop(self.join.open)
                        .on_click(
                            cx.listener(|shell, _, window, cx| shell.submit_join(window, cx)),
                        ),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_link_says_what_to_paste_instead() {
        assert_eq!(Problem::of("  ", InviteError::Scheme), Problem::Empty);
        assert_eq!(
            Problem::of("https://example.com", InviteError::Scheme),
            Problem::NotInvite
        );
        assert_eq!(
            Problem::of("panorama://party/abc", InviteError::Shape),
            Problem::Broken
        );
        assert!(Problem::Empty.text().contains("invite link"));
    }
}
