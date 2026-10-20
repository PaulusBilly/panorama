//! The Subtitles panel: language, track and appearance columns, or tabs when narrow.
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
    theme::Theme,
};
use gpui::{AnyElement, Div, FontWeight, Window, div, prelude::*, px, relative};

/// The container query of the TSX: columns from 36 rem of panel content.
pub(super) const WIDE_AT: f32 = 576.0;
/// `p-2` around the page.
const PAGE_PAD: f32 = 8.0;
/// The column share of `0.8fr 1.1fr 1.1fr`.
const SHARES: [f32; 3] = [0.8, 1.1, 1.1];

/// Whether a page of this width shows three columns.
pub(super) fn is_wide(page_width: f32) -> bool {
    page_width - 2.0 * PAGE_PAD >= WIDE_AT
}

/// Pixel widths of the three columns for a page width; dividers sit inside the columns.
pub(super) fn column_widths(page_width: f32) -> [f32; 3] {
    let total = page_width - 2.0 * PAGE_PAD;
    SHARES.map(|share| total * share / 3.0)
}

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
    fn label_row(&self, text: &'static str, theme: Theme) -> Div {
        // `PlayerSectionLabel`: caption, 12 px sides, 8 px above and 4 below.
        note(text, theme, 0.55).px(px(12.0)).pt(px(8.0)).pb(px(4.0))
    }

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
            Some(text) => list.child(caption(&text.text, theme, false).px(px(12.0)).py(px(12.0))),
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
            .px(px(12.0))
            .pb(px(8.0))
            .pt(px(4.0));
        if let Some(limitation) = limitation {
            body = body.child(note(limitation.message(), theme, 0.7));
        } else {
            let look = Appearance::from(&self.ui.prefs);
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .min_w_0()
                    .child(note("Sample", theme, 0.55).mb(px(8.0)))
                    .child(styled_cue("\u{266a} A song begins \u{266a}", &look)),
            );
        }
        body = body.child(self.color_controls(limited, theme, keyboard, window, cx));
        for (index, field) in Field::ALL.into_iter().enumerate() {
            let disabled = limited && !field.works_when_limited();
            body = body.child(self.stepper(index, field, disabled, theme, keyboard, window, cx));
        }
        body.child(self.weight_controls(limited, theme, keyboard, window, cx))
    }

    fn tab_row(
        &self,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Div {
        let ink = theme.ink;
        let items = Tab::ALL.iter().enumerate().map(|(index, (tab, label))| {
            let tab = *tab;
            let active = self.ui.subs.tab == tab;
            let item = div()
                .id(("subtitle-tab", index))
                .role(gpui::Role::Tab)
                .aria_label(*label)
                .flex_1()
                .min_h(px(40.0))
                .px(px(8.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .line_height(relative(1.43))
                .cursor_pointer()
                .when(active, |item| item.bg(ink.opacity(0.15)))
                .when(!active, |item| item.text_color(ink.opacity(0.7)))
                .on_click(cx.listener(move |player, _, _, cx| {
                    player.ui.subs.tab = tab;
                    cx.notify();
                }))
                .child(*label);
            crate::theme::focus_ring(
                item,
                &self.focus.pool[super::panel_widgets::slot::TABS + index],
                super::widgets::focus_theme(theme),
                true,
                keyboard,
                window,
            )
        });
        div()
            .flex()
            .gap(px(4.0))
            .mb(px(4.0))
            .rounded(px(12.0))
            .bg(ink.opacity(0.1))
            .p(px(4.0))
            .children(items)
    }

    /// The body under the title of the Subtitles page.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn subtitles_body(
        &mut self,
        content: &Content,
        width: f32,
        highlight: Option<usize>,
        theme: Theme,
        keyboard: bool,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let ink = theme.ink;
        let languages = content.sections[0].rows.len();
        if is_wide(width) {
            let widths = column_widths(width);
            let column = |index: usize, label: &'static str, pad: [f32; 2], body: AnyElement| {
                let scroll = self.scrolls[4 + index].clone();
                div()
                    .w(px(widths[index]))
                    .flex_shrink_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when(index > 0, |column| {
                        column.border_l_1().border_color(ink.opacity(0.1))
                    })
                    .child(self.label_row(label, theme).bg(theme.player_canvas))
                    .child(
                        div()
                            .id(("subtitle-column", index))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .pl(px(pad[0]))
                            .pr(px(pad[1]))
                            .child(body),
                    )
            };
            let lang = self
                .section_rows(content, 0, 0, highlight, theme, cx)
                .into_any_element();
            let tracks = self
                .section_rows(content, 1, languages, highlight, theme, cx)
                .into_any_element();
            let appearance = self
                .appearance_body(theme, keyboard, window, cx)
                .into_any_element();
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .px(px(PAGE_PAD))
                .pb(px(PAGE_PAD))
                .child(column(0, "Language", [0.0, 4.0], lang))
                .child(column(1, "Track", [4.0, 4.0], tracks))
                .child(column(2, "Appearance", [4.0, 0.0], appearance))
                .into_any_element();
        }
        let tab = self.ui.subs.tab;
        let body = match tab {
            Tab::Languages => self.section_rows(content, 0, 0, highlight, theme, cx),
            Tab::Variants => self.section_rows(content, 1, languages, highlight, theme, cx),
            Tab::Settings => self.appearance_body(theme, keyboard, window, cx),
        };
        let scroll = self.scrolls[4].clone();
        div()
            .id("subtitle-narrow")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(PAGE_PAD))
            .pb(px(PAGE_PAD))
            .child(self.tab_row(theme, keyboard, window, cx))
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_switches_at_576_px_of_content() {
        assert!(!is_wide(591.0));
        assert!(is_wide(592.0));
        assert!(is_wide(736.0));
        // A 760 px window gives a 720 px page (window - 40): still three columns.
        assert!(is_wide(720.0));
    }

    #[test]
    fn columns_split_0_8_1_1_1_1() {
        let [a, b, c] = column_widths(736.0);
        assert!((a + b + c - 720.0).abs() < 1e-3);
        assert!((a / b - 0.8 / 1.1).abs() < 1e-4);
        assert_eq!(b, c);
    }
}
