use crate::logic::Tween;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, Div, FocusHandle, MouseButton, Window, div,
    prelude::*, px, rgb,
};
use std::time::{Duration, Instant};

pub struct Motion {
    pub page_b: bool,
    pages: [(Tween, Tween); 2],
    pub dialog_open: bool,
    pub dialog_present: bool,
    panel: Tween,
    scrim: Tween,
    revision: usize,
    pub opener: FocusHandle,
    pub close: FocusHandle,
    pub stay: FocusHandle,
    return_focus: Option<FocusHandle>,
}

fn animated<E: IntoElement + 'static>(
    element: E,
    id: (&'static str, usize),
    tween: Tween,
    apply: impl Fn(E, f32) -> E + 'static,
) -> AnyElement {
    let animation = Animation::new(tween.duration.max(Duration::from_millis(1)));
    element
        .with_animation(id, animation, move |element, _| {
            apply(element, tween.value(Instant::now()))
        })
        .into_any_element()
}

impl Motion {
    pub fn new(cx: &mut Context<crate::Gate>) -> Self {
        Self {
            page_b: false,
            pages: [
                (Tween::fixed(1.0), Tween::fixed(0.0)),
                (Tween::fixed(0.0), Tween::fixed(16.0)),
            ],
            dialog_open: false,
            dialog_present: false,
            panel: Tween::fixed(0.0),
            scrim: Tween::fixed(0.0),
            revision: 0,
            opener: cx.focus_handle(),
            close: cx.focus_handle(),
            stay: cx.focus_handle(),
            return_focus: None,
        }
    }
    pub fn navigate(&mut self, to_b: bool, now: Instant) {
        if self.page_b == to_b {
            return;
        }
        self.page_b = to_b;
        self.revision += 1;
        for (index, (alpha, y)) in self.pages.iter_mut().enumerate() {
            let incoming = index == usize::from(to_b);
            if incoming && alpha.value(now) == 0.0 && !alpha.active(now) {
                *y = Tween::fixed(16.0);
            }
            let duration = Duration::from_millis(if incoming { 320 } else { 160 });
            alpha.retarget(if incoming { 1.0 } else { 0.0 }, duration, incoming, now);
            y.retarget(if incoming { 0.0 } else { -12.0 }, duration, incoming, now);
        }
    }
    pub fn dialog(
        &mut self,
        open: bool,
        window: &mut Window,
        cx: &mut Context<crate::Gate>,
        now: Instant,
    ) {
        if self.dialog_open == open {
            return;
        }
        self.revision += 1;
        self.dialog_open = open;
        if open {
            if !self.dialog_present {
                self.return_focus = window.focused(cx).or_else(|| Some(self.opener.clone()));
            }
            self.dialog_present = true;
            window.focus(&self.close, cx);
        }
        let duration = Duration::from_millis(if open { 220 } else { 160 });
        self.panel.retarget(f32::from(open), duration, true, now);
        self.scrim.retarget(f32::from(open), duration, false, now);
    }
    pub fn finish_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<crate::Gate>,
        now: Instant,
    ) {
        if self.dialog_present && !self.dialog_open && !self.panel.active(now) {
            self.dialog_present = false;
            if let Some(handle) = self.return_focus.take() {
                window.focus(&handle, cx);
            }
        }
    }
    pub fn tab(&self, window: &mut Window, cx: &mut Context<crate::Gate>) {
        window.focus(
            if self.close.is_focused(window) {
                &self.stay
            } else {
                &self.close
            },
            cx,
        );
    }
    pub fn render(&self, _: &Window, cx: &mut Context<crate::Gate>) -> Div {
        let now = Instant::now();
        let mut surface = div().relative().size_full().overflow_hidden();
        for (index, (alpha, y)) in self.pages.iter().copied().enumerate() {
            if alpha.value(now) == 0.0 && !alpha.active(now) {
                continue;
            }
            let page = if index == 0 {
                div()
                    .child(
                        div()
                            .text_size(px(36.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("An evening at the cinema"),
                    )
                    .child(
                        div()
                            .mt(px(12.0))
                            .child("Enter or click to view film details · D opens the dialog"),
                    )
                    .child(
                        div()
                            .mt(px(48.0))
                            .flex()
                            .gap(px(24.0))
                            .children((0..6).map(|_| {
                                div()
                                    .w(px(180.0))
                                    .h(px(270.0))
                                    .rounded(px(6.0))
                                    .border_1()
                                    .border_color(rgb(0xffffff).opacity(0.1))
                                    .bg(rgb(0x2b2a27))
                            })),
                    )
            } else {
                div().child(div().text_size(px(52.0)).font_weight(gpui::FontWeight::BOLD).line_height(px(54.6)).child("The Grand Budapest Hotel"))
                    .child(div().mt(px(24.0)).text_size(px(18.0)).child("2014 · 1h 39m · ★ 8.1/10"))
                    .child(div().mt(px(36.0)).w(px(720.0)).text_size(px(16.0)).line_height(px(24.8)).child("A legendary concierge and his young protégé become friends in a grand European hotel. A stolen painting, a family fortune, and a changing world set their extraordinary adventure in motion."))
                    .child(div().mt(px(32.0)).child("Backspace / Esc to return · D opens the dialog"))
            };
            let target = !self.page_b;
            let page = page
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .p(px(64.0))
                .bg(rgb(0x1a1917))
                .id(if index == 0 { "page-a" } else { "page-b" })
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.motion.opener, cx);
                    this.motion.navigate(target, Instant::now());
                    cx.notify();
                }));
            let revision = self.revision;
            surface = surface.child(animated(
                page,
                (
                    if index == 0 {
                        "page-a-animation"
                    } else {
                        "page-b-animation"
                    },
                    revision,
                ),
                alpha,
                move |page, amount| page.opacity(amount).top(px(y.value(Instant::now()))),
            ));
        }
        surface = surface.child(
            div()
                .id("dialog-opener")
                .track_focus(&self.opener)
                .absolute()
                .bottom(px(48.0))
                .left(px(64.0))
                .p(px(12.0))
                .border_1()
                .rounded(px(6.0))
                .border_color(rgb(0xeeedea).opacity(0.3))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.motion.opener, cx);
                    this.motion.dialog(true, window, cx, Instant::now());
                    cx.stop_propagation();
                    cx.notify();
                }))
                .child("Open dialog (D)"),
        );
        surface
    }
    pub fn overlay(&self, window: &Window, cx: &mut Context<crate::Gate>) -> Div {
        let now = Instant::now();
        let mut surface = div().absolute().inset_0();
        if self.dialog_present {
            let width = f32::from(window.viewport_size().width);
            let height = f32::from(window.viewport_size().height);
            let panel = self.panel;
            let scrim = self.scrim;
            let overlay = div()
                .id("dialog-scrim")
                .absolute()
                .inset_0()
                .occlude()
                .bg(rgb(0x0d0d0d).opacity(0.72))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.motion.dialog(false, window, cx, Instant::now());
                    cx.stop_propagation();
                    cx.notify();
                }));
            surface = surface.child(animated(
                overlay,
                ("scrim-animation", self.revision),
                scrim,
                |el, amount| el.opacity(amount),
            ));
            let value = panel.value(now);
            let scale = 0.96 + 0.04 * value;
            let contents = div().id("dialog-panel").absolute().occlude().rounded(px(12.0)).bg(rgb(0x2b2a27)).border_1().border_color(rgb(0xffffff).opacity(0.12)).p(px(32.0 * scale))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(div().text_size(px(24.0 * scale)).font_weight(gpui::FontWeight::BOLD).child("A moment before the film"))
                .child(div().mt(px(20.0 * scale)).text_size(px(16.0 * scale)).line_height(px(24.8 * scale)).child("Hand-built presence, opacity, scale and focus. Tab cycles between the two buttons. D reverses the animation."))
                .child(div().absolute().bottom(px(32.0 * scale)).left(px(32.0 * scale)).flex().gap(px(16.0 * scale)).text_size(px(13.0 * scale))
                    .child(div().id("dialog-stay").track_focus(&self.stay).p(px(10.0 * scale)).border_1().rounded(px(6.0)).focus(|style| style.border_color(rgb(0xeeedea))).on_click(cx.listener(|this, _, window, cx| { window.focus(&this.motion.stay, cx); cx.stop_propagation(); })).child("Stay"))
                    .child(div().id("dialog-close").track_focus(&self.close).p(px(10.0 * scale)).border_1().rounded(px(6.0)).cursor_pointer().focus(|style| style.border_color(rgb(0xeeedea)))
                        .on_click(cx.listener(|this, _, window, cx| { this.motion.dialog(false, window, cx, Instant::now()); cx.stop_propagation(); cx.notify(); })).child("Close")));
            surface = surface.child(animated(
                contents,
                ("panel-animation", self.revision),
                panel,
                move |el, amount| {
                    let scale = 0.96 + 0.04 * amount;
                    el.opacity(amount)
                        .w(px(480.0 * scale))
                        .h(px(320.0 * scale))
                        .left(px((width - 480.0 * scale) / 2.0))
                        .top(px((height - 320.0 * scale) / 2.0))
                },
            ));
        }
        surface
    }
}
