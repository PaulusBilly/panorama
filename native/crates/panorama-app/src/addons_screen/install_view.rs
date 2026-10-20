//! "Install from a link": the input, its error line and the confirmation preview.
use super::{
    Addons, Slot,
    model::{InstallFlow, PreviewCard, error_copy},
    widgets::{View, logo, section, tnum},
};
use crate::assets::Weighted;
use crate::{
    a11y,
    button::Button,
    image_cache::CachedImage,
    theme::{Typography, motion::*},
};
use gpui::{Context, Div, FontWeight, Window, canvas, div, prelude::*, px, relative};
use gpui_component::input::Input;
use std::time::Instant;
use url::Url;

impl Addons {
    pub(super) fn logo_image(
        &mut self,
        url: Option<&Url>,
        size: f32,
        view: View,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<CachedImage> {
        let url = url.filter(|_| view.active)?;
        let services = self.state.read(cx).services.clone()?;
        let scale = window.scale_factor();
        if self.images.update(cx, |images, _| {
            images.failed(url.as_str(), (size, size), scale)
        }) {
            return None;
        }
        self.images.update(cx, |images, cx| {
            images.request(url.as_str(), (size, size), &services, window, cx)
        })
    }

    pub(super) fn install_section(
        &mut self,
        view: View,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let now = Instant::now();
        let previewing = matches!(self.flow, InstallFlow::Previewing { .. });
        let preview_focus = self.handle(Slot::Preview, cx);
        let input_focus = gpui::Focusable::focus_handle(&self.input, cx);
        let focused = input_focus.is_focused(window);
        let trigger = div().absolute().right_0().bottom(px(4.0)).child(
            Button::glass(
                "addon-preview",
                if previewing { "Checking" } else { "Preview" },
            )
            .compact()
            .busy(previewing)
            .reduced_motion(view.reduced)
            .focus(&preview_focus, view.keyboard)
            .tab_stop(view.active)
            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
        );
        let input = div()
            .relative()
            .mt(px(16.0))
            .w_full()
            .min_h(px(48.0))
            .pb(px(if focused { 9.0 } else { 10.0 }))
            .border_b(px(if focused { 2.0 } else { 1.0 }))
            .border_color(theme.ink)
            .flex()
            .flex_col()
            .justify_end()
            .child(
                Input::new(&self.input)
                    .appearance(false)
                    .bordered(false)
                    .focus_bordered(false)
                    .aria_label("Addon link")
                    .map(|input| gpui::Styled::h(input, px(28.0)))
                    .p_0()
                    .pr(px(120.0))
                    .text_size(px(24.0))
                    .weight(FontWeight::MEDIUM)
                    .line_height(relative(1.15))
                    .text_color(theme.ink),
            )
            .child(trigger);
        let mut section = section(view, "Install from a link", None).child(input);
        if let InstallFlow::Failed(error) = &self.flow {
            section = section.child(
                div()
                    .id("addon-error")
                    .mt(px(8.0))
                    .label()
                    .text_color(theme.danger)
                    .role(gpui::Role::Alert)
                    .child(error_copy(error)),
            );
        }
        if let Some(text) = self.notice.clone() {
            section = section
                .child(
                    div()
                        .mt(px(8.0))
                        .label()
                        .text_color(theme.ink)
                        .child(text.clone()),
                )
                .child(a11y::status("addon-status", text));
        }
        let card = match &self.flow {
            InstallFlow::Preview { card, .. } | InstallFlow::Installing { card } => {
                Some(card.clone())
            }
            _ => None,
        };
        if let Some(card) = &card {
            self.shown = Some(card.clone());
        }
        self.reveal.retarget(
            if card.is_some() { 1.0 } else { 0.0 },
            DURATION_STANDARD,
            Some(EASE_EDITORIAL),
            now,
        );
        let progress = self.reveal.value(now, view.reduced);
        if card.is_none() && progress <= 0.0 {
            self.shown = None;
        }
        match self.shown.clone() {
            Some(card) => section.child(self.preview_block(card, progress, view, window, cx)),
            None => section,
        }
    }

    fn preview_block(
        &mut self,
        card: PreviewCard,
        progress: f32,
        view: View,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let installing = matches!(self.flow, InstallFlow::Installing { .. });
        let image = self.logo_image(card.logo.as_ref(), 64.0, view, window, cx);
        let labels: Vec<String> = card
            .capabilities
            .iter()
            .map(|capability| capability.label().to_uppercase())
            .collect();
        let types: Vec<String> = card
            .types
            .iter()
            .map(|label| label.to_uppercase())
            .collect();
        let details = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .title()
                    .text_color(theme.ink)
                    .truncate()
                    .child(card.name.clone()),
            )
            .child(
                div()
                    .caption()
                    .font_features(tnum())
                    .text_color(theme.ink_muted)
                    .child(format!("Version {}", card.version)),
            )
            .when_some(card.description.clone(), |column, text| {
                column.child(
                    div()
                        .mt(px(12.0))
                        .body()
                        .text_color(theme.ink)
                        .max_w(px(590.0))
                        .line_clamp(4)
                        .child(text),
                )
            })
            .child(
                div()
                    .mt(px(12.0))
                    .flex()
                    .flex_wrap()
                    .gap_x(px(16.0))
                    .label()
                    .text_color(theme.ink_muted)
                    .children(labels)
                    .children(types),
            );
        let block = div()
            .p(px(24.0))
            .border_1()
            .border_color(theme.rule)
            .bg(theme.canvas)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .gap(px(20.0))
                    .child(logo(64.0, &card.name, image, true, theme))
                    .child(details),
            )
            .child(
                div()
                    .mt(px(24.0))
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .child(self.install_button(installing, view, cx))
                    .when(!installing, |row| {
                        let focus = self.handle(Slot::Cancel, cx);
                        row.child(
                            Button::glass("addon-cancel", "Cancel")
                                .focus(&focus, view.keyboard)
                                .tab_stop(view.active)
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_preview(cx))),
                        )
                    }),
            );
        let measured = self.measured.clone();
        let natural = measured.get();
        div()
            .mt(px(24.0 * progress))
            .overflow_hidden()
            .opacity(progress)
            .when(progress < 1.0 && natural > 0.0, |wrapper| {
                wrapper.h(px(natural * progress))
            })
            .child(
                div()
                    .relative()
                    .flex_none()
                    .child(
                        canvas(
                            move |bounds, _, _| measured.set(f32::from(bounds.size.height)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(block),
            )
    }

    fn install_button(&mut self, installing: bool, view: View, cx: &mut Context<Self>) -> Button {
        let focus = self.handle(Slot::Install, cx);
        Button::primary(
            "addon-install",
            if installing {
                "Installing"
            } else {
                "Install addon"
            },
        )
        .busy(installing)
        .reduced_motion(view.reduced)
        .focus(&focus, view.keyboard)
        .tab_stop(view.active)
        .on_click(cx.listener(|this, _, _, cx| this.confirm_install(cx)))
    }
}
