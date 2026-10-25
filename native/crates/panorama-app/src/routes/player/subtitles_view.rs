//! The Subtitles panel: language, track and appearance tabs.
use super::{
    Player,
    content::{Content, Section},
    panel_view::caption,
    panel_widgets::{color, note},
    subtitles::Tab,
};
use crate::assets::Weighted;
use crate::{
    playback::{subtitle_prefs::Field, subtitle_render::Appearance},
    theme::{
        Theme,
        motion::{DURATION_STANDARD, EASE_EDITORIAL},
    },
};
use gpui::{AnyElement, Div, FontWeight, Window, div, prelude::*, px, relative};
use std::time::Instant;

/// `p-2` around the page.
const PAGE_PAD: f32 = 8.0;
/// How far a page slides per tab it travels.
const TAB_SLIDE: f32 = 16.0;

fn with_alpha(rgb: [u8; 3], alpha: f32) -> gpui::Rgba {
    let mut value = color(rgb);
    value.a = alpha.clamp(0.0, 1.0);
    value
}

/// A cue styled by the preferences: text colour, box colour, padding, rounding and type.
pub(super) fn styled_cue(text: &str, look: &Appearance) -> Div {
    div()
        .max_w_full()
        .px(px(look.padding_x))
        .py(px(look.padding_y))
        .rounded(px(look.radius))
        .bg(with_alpha([0, 0, 0], look.background_alpha))
        .text_color(with_alpha(look.text.0, look.text.1))
        .text_size(px(look.font_px))
        .weight(FontWeight(look.weight))
        .line_height(relative(look.line_height))
        .text_center()
        .child(text.to_owned())
}

impl Player {
    fn section_rows(
        &self,
        content: &Content,
        section: usize,
        offset: usize,
        highlight: Option<usize>,
        theme: Theme,
        cx: &mut gpui::Context<Self>,
    ) -> Div {
        let Section { rows, note, .. } = &content.sections[section];
        let mut list = div().flex().flex_col();
        for (index, row) in rows.iter().enumerate() {
            list = list.child(self.option_row(
                row,
                offset + index,
                highlight == Some(offset + index),
                theme,
                cx,
            ));
        }
        match note {
            Some(text) => list.child(caption(&text.text, theme, false).px(px(20.0)).py(px(12.0))),
            None => list,
        }
    }

    fn appearance_body(
        &self,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Div {
        let renderer = self.renderer();
        let limitation = renderer.limitation();
        let limited = limitation.is_some();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .px(px(20.0))
            .pb(px(8.0))
            .pt(px(4.0));
        if let Some(limitation) = limitation {
            body = body.child(note(limitation.message(), theme, 0.7));
        }
        body = body.child(self.color_controls(limited, theme, keyboard, window, cx));
        for (index, field) in Field::ALL.into_iter().enumerate() {
            let disabled = limited && !field.works_when_limited();
            body = body.child(self.stepper(index, field, disabled, theme, keyboard, window, cx));
        }
        body.child(self.weight_controls(limited, theme, keyboard, window, cx))
    }

    /// Switch tabs: the indicator glides and the new page slides in from its side.
    fn set_tab(&mut self, tab: Tab, cx: &mut gpui::Context<Self>) {
        if self.ui.subs.tab == tab {
            return;
        }
        self.ui.subs.tab = tab;
        self.tab_motion.retarget(
            tab.index() as f32,
            DURATION_STANDARD,
            Some(EASE_EDITORIAL),
            Instant::now(),
        );
        self.scrolls[4].set_offset(gpui::point(px(0.0), px(0.0)));
        cx.notify();
    }

    fn tab_row(
        &self,
        at: f32,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Div {
        let ink = theme.ink;
        let count = Tab::ALL.len() as f32;
        let items = Tab::ALL.iter().enumerate().map(|(index, (tab, label))| {
            let tab = *tab;
            let active = self.ui.subs.tab == tab;
            let item = div()
                .id(("subtitle-tab", index))
                .role(gpui::Role::Tab)
                .aria_label(*label)
                .relative()
                .flex_1()
                .min_h(px(40.0))
                .px(px(4.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .line_height(relative(1.43))
                .cursor_pointer()
                .when(!active, |item| {
                    item.text_color(ink.opacity(0.55))
                        .hover(move |style| style.text_color(ink))
                })
                .on_click(cx.listener(move |player, _, _, cx| player.set_tab(tab, cx)))
                .child(*label);
            crate::theme::focus_ring(
                item,
                &self.focus.pool[super::panel_widgets::slot::TABS + index],
                super::widgets::focus_theme(theme),
                true,
                keyboard,
                0.0,
                window,
            )
        });
        // A hairline under the row, and the active tab's underline gliding along it.
        let rule = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(1.0))
            .bg(ink.opacity(0.1));
        let indicator = div()
            .absolute()
            .bottom_0()
            .h(px(2.0))
            .left(relative(at / count))
            .w(relative(1.0 / count))
            .bg(ink);
        div()
            .relative()
            .flex_shrink_0()
            .flex()
            .mx(px(20.0))
            .mb(px(12.0))
            .child(rule)
            .children(items)
            .child(indicator)
    }

    /// The body under the title of the Subtitles page.
    pub(super) fn subtitles_body(
        &mut self,
        content: &Content,
        highlight: Option<usize>,
        theme: Theme,
        keyboard: bool,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let languages = content.sections[0].rows.len();
        let tab = self.ui.subs.tab;
        let at = self.tab_motion.value(Instant::now(), self.reduced);
        let body = match tab {
            Tab::Languages => self.section_rows(content, 0, 0, highlight, theme, cx),
            Tab::Variants => self.section_rows(content, 1, languages, highlight, theme, cx),
            Tab::Settings => self.appearance_body(theme, keyboard, window, cx),
        };
        // How far the page still has to travel, in tabs: it slides in from that side and fades.
        let away = at - tab.index() as f32;
        let scroll = self.scrolls[4].clone();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .px(px(0.0))
            .child(self.tab_row(at, theme, keyboard, window, cx))
            .child(
                div()
                    .id("subtitle-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .pb(px(PAGE_PAD))
                    .child(
                        body.relative()
                            .left(px(away * TAB_SLIDE))
                            .opacity(1.0 - away.abs().min(1.0)),
                    ),
            )
            .into_any_element()
    }
}
