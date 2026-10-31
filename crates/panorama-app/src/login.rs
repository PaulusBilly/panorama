use crate::{
    app::AppShell,
    app_state::{Account, AppState},
    button::{Button, Icon},
    theme::{Theme, Typography, motion::*},
    transition::Tween,
};
use gpui::{
    BoxShadow, Context, Entity, FocusHandle, Focusable, Render, Subscription, WeakEntity, Window,
    div, point, prelude::*, px, relative,
};
use gpui_component::input::{Input, InputContentType, InputState};
use std::time::Instant;

gpui::actions!(
    panorama_login,
    [NextField, PreviousField, CloseDialog, Submit]
);

/// Sign-in form with component-owned editing and modal focus trapping.
pub struct LoginDialog {
    shell: WeakEntity<AppShell>,
    state: Entity<AppState>,
    email: Entity<InputState>,
    password: Entity<InputState>,
    close: FocusHandle,
    submit: FocusHandle,
    presence: Tween,
    scrim: Tween,
    closing: bool,
    signed_in: bool,
    _account: Subscription,
}
impl LoginDialog {
    /// Create a fresh form and focus Email; the scrim does not dismiss it.
    pub fn new(
        shell: WeakEntity<AppShell>,
        state: Entity<AppState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let email = cx.new(|cx| InputState::new(window, cx));
        let password = cx.new(|cx| InputState::new(window, cx).masked(true));
        email.focus_handle(cx).focus(window, cx);
        let observer = cx.observe_in(&state, window, |dialog, state, window, cx| {
            let signed_in = matches!(state.read(cx).account, Account::SignedIn(_));
            if signed_in && !dialog.signed_in {
                dialog.close(window, cx);
            }
            dialog.signed_in = signed_in;
            cx.notify();
        });
        let mut presence = Tween::fixed(0.0);
        presence.retarget(1.0, DURATION_STANDARD, Some(EASE_EDITORIAL), Instant::now());
        let mut scrim = Tween::fixed(0.0);
        scrim.retarget(1.0, DURATION_FAST, None, Instant::now());
        Self {
            shell,
            state: state.clone(),
            email,
            password,
            close: cx.focus_handle(),
            submit: cx.focus_handle(),
            presence,
            scrim,
            closing: false,
            signed_in: matches!(state.read(cx).account, Account::SignedIn(_)),
            _account: observer,
        }
    }
    fn clear(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.email
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.password
            .update(cx, |input, cx| input.set_value("", window, cx));
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.clear(window, cx);
        self.state.update(cx, |state, cx| state.cancel_sign_in(cx));
        self.presence
            .retarget(0.0, DURATION_FAST, Some(EASE_EDITORIAL), Instant::now());
        self.scrim
            .retarget(0.0, DURATION_FAST, None, Instant::now());
        cx.notify();
    }
    fn cycle(&self, backward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut handles = vec![
            self.close.clone(),
            self.email.focus_handle(cx),
            self.password.focus_handle(cx),
            self.submit.clone(),
        ];
        if matches!(self.state.read(cx).account, Account::Authenticating) {
            handles.pop();
        }
        let current = handles
            .iter()
            .position(|handle| handle.is_focused(window))
            .unwrap_or(1);
        let index = (current + if backward { handles.len() - 1 } else { 1 }) % handles.len();
        handles[index].focus(window, cx);
        cx.stop_propagation();
    }
    fn sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.state.read(cx).account, Account::Authenticating) {
            return;
        }
        let email = self.email.read(cx).value().to_string();
        let password = self.password.read(cx).value().to_string();
        if email.trim().is_empty() || password.is_empty() {
            self.email.focus_handle(cx).focus(window, cx);
            return;
        }
        self.clear(window, cx);
        self.state
            .update(cx, |state, cx| state.sign_in(email, password, cx));
        cx.stop_propagation();
    }
    fn field(
        &self,
        label: &'static str,
        input: &Entity<InputState>,
        short: bool,
        theme: Theme,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let focused = input.focus_handle(cx).is_focused(window);
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .text_size(px(13.0))
            .line_height(relative(1.4))
            .text_color(theme.ink_muted)
            .child(label)
            .child(
                div()
                    .border_b(px(if focused { 2.0 } else { 1.0 }))
                    .border_color(theme.ink)
                    .py(px(if short { 8.0 } else { 11.0 }))
                    .child(
                        Input::new(input)
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .content_type(if label == "Email" {
                                InputContentType::EmailAddress
                            } else {
                                InputContentType::Password
                            })
                            .aria_label(label)
                            .map(|input| gpui::Styled::h(input, px(24.0)))
                            .p_0()
                            .text_size(px(16.0))
                            .line_height(relative(1.5))
                            .text_color(theme.ink),
                    ),
            )
    }
}
impl Render for LoginDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (theme, reduced, keyboard) = self
            .shell
            .upgrade()
            .map(|shell| {
                let shell = shell.read(cx);
                (shell.theme, shell.reduced_motion, shell.keyboard_navigation)
            })
            .unwrap_or((Theme::default(), false, true));
        let now = Instant::now();
        let progress = self.presence.value(now, reduced);
        let scrim = self.scrim.value(now, reduced);
        if self.presence.moving(now, reduced) || self.scrim.moving(now, reduced) || self.closing {
            cx.on_next_frame(window, |dialog, window, cx| {
                let reduced = dialog
                    .shell
                    .upgrade()
                    .is_some_and(|shell| shell.read(cx).reduced_motion);
                if dialog.closing && !dialog.presence.moving(Instant::now(), reduced) {
                    let shell = dialog.shell.clone();
                    let _ = shell.update(cx, |shell, cx| shell.finish_login(window, cx));
                } else {
                    cx.notify();
                }
            });
            window.request_animation_frame();
        }
        let width = f32::from(window.viewport_size().width);
        let short = f32::from(window.viewport_size().height) < 560.0;
        let busy = matches!(self.state.read(cx).account, Account::Authenticating);
        let error = match &self.state.read(cx).account {
            Account::Error(message) => Some(message.clone()),
            _ => None,
        };
        let panel = div()
            .relative()
            .w(px((width - 40.0).min(520.0)))
            .px(px(if width < 560.0 { 24.0 } else { 34.0 }))
            .py(px(if short { 20.0 } else { 30.0 }))
            .bg(theme.surface)
            .text_color(theme.ink)
            .opacity(progress)
            .top(px(if reduced {
                0.0
            } else {
                12.0 * (1.0 - progress)
            }))
            .flex()
            .flex_col()
            .gap(px(if short { 16.0 } else { 24.0 }))
            .shadow(vec![BoxShadow {
                inset: false,
                color: theme.shadow.into(),
                offset: point(px(0.0), px(24.0)),
                blur_radius: px(80.0),
                spread_radius: px(0.0),
            }])
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(24.0))
                    .pb(px(if short { 16.0 } else { 24.0 }))
                    .border_b_1()
                    .border_color(theme.rule)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .title()
                            .child("Sign in to sync addons"),
                    )
                    .child(
                        Button::round("login-close", Icon::Close, "Close sign in")
                            .compact()
                            .focus(&self.close, keyboard)
                            .tab_stop(!self.closing)
                            .on_click(
                                cx.listener(|dialog, _, window, cx| dialog.close(window, cx)),
                            ),
                    ),
            )
            .child(self.field("Email", &self.email, short, theme, window, cx))
            .child(self.field("Password", &self.password, short, theme, window, cx))
            .when_some(error, |panel, error| {
                panel.child(
                    div()
                        .id("login-error")
                        .caption()
                        .text_color(theme.danger)
                        .role(gpui::Role::Alert)
                        .child(error),
                )
            })
            .child(
                div().flex().justify_end().child(
                    Button::primary("login-submit", if busy { "Signing in" } else { "Sign in" })
                        .aria_label("Sign in")
                        .compact()
                        .busy(busy)
                        .reduced_motion(reduced)
                        .focus(&self.submit, keyboard)
                        .tab_stop(!self.closing)
                        .on_click(cx.listener(|dialog, _, window, cx| dialog.sign_in(window, cx))),
                ),
            );
        div()
            .id("login-dialog")
            .absolute()
            .inset_0()
            .occlude()
            .key_context("PanoramaLogin")
            .role(gpui::Role::Dialog)
            .aria_label("Sign in to sync addons")
            .on_action(
                cx.listener(|dialog, _: &NextField, window, cx| dialog.cycle(false, window, cx)),
            )
            .on_action(
                cx.listener(|dialog, _: &PreviousField, window, cx| dialog.cycle(true, window, cx)),
            )
            .on_action(cx.listener(|dialog, _: &CloseDialog, window, cx| dialog.close(window, cx)))
            .on_action(cx.listener(|dialog, _: &Submit, window, cx| {
                if dialog.close.is_focused(window) {
                    dialog.close(window, cx);
                    cx.stop_propagation();
                } else {
                    dialog.sign_in(window, cx);
                }
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(theme.scrim.opacity(0.72))
                    .opacity(scrim),
            )
            .child(
                div()
                    .relative()
                    .size_full()
                    .pt(px(crate::titlebar::SAFE_TOP))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(panel),
            )
    }
}
