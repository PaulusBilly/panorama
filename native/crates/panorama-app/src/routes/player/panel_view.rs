//! The shared panel surface, its pages and option rows.
use super::{
    Player,
    content::{Content, Item, Row},
    measure::probe,
    panel::PanelId,
    panel_motion::{incoming, outgoing},
    widgets::{ring, tabular},
};
use crate::theme::{Theme, Typography};
use gpui::{
    AnyElement, Context, Div, ScrollHandle, Stateful, Window, div, prelude::*, px, relative,
};
use std::time::Instant;

/// Height of a section label: caption line plus its 12 px top and 4 px bottom padding.
const LABEL_HEIGHT: f32 = 11.0 * 1.4 + 12.0 + 4.0;
/// A stuck label sits 8 px above the list's top edge, under the sticky header.
const STICK_OFFSET: f32 = -8.0;

/// Which section label sticks and where, given each label's natural top (all in window space).
pub(super) fn sticky_label(view_top: f32, tops: &[f32]) -> Option<(usize, f32)> {
    let line = view_top + STICK_OFFSET;
    let active = tops.iter().rposition(|top| *top < line)?;
    let top = tops
        .get(active + 1)
        .map_or(line, |next| line.min(next - LABEL_HEIGHT));
    Some((active, top - view_top))
}

impl Player {
    /// The surface with its page or pages, or `None` once closed and settled.
    pub(super) fn panel_surface(
        &mut self,
        theme: Theme,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let now = Instant::now();
        let reduced = self.reduced;
        self.measures.surface.set(None);
        let displayed = self.panels.displayed()?;
        self.panel_motion
            .tick(self.measures.pages[displayed as usize].get(), now, reduced);
        let appear = self.panel_motion.appear(now, reduced);
        if self.panels.active().is_none() && appear <= 0.001 {
            self.panels.settle();
            return None;
        }
        let offset = self.panel_motion.offset(now, reduced);
        let mut surface = div()
            .id("player-panel")
            .role(gpui::Role::Dialog)
            .absolute()
            .right(px(20.0))
            .bottom(px(self.panel_motion.bottom - offset))
            .rounded(px(16.0))
            .bg(theme.player_canvas)
            .text_color(theme.ink)
            .overflow_hidden()
            .opacity(appear)
            .occlude()
            .track_focus(&self.focus.panel);
        surface = surface.child(probe(&self.measures, |measures, bounds| {
            measures.surface.set(Some(bounds));
        }));
        match (
            self.panel_motion.switching(),
            self.panel_motion.pose(now, reduced),
        ) {
            (Some(switch), Some(pose)) => {
                surface = surface.w(pose.size.width).h(pose.size.height);
                if let Some(page) = self.panel_page(
                    switch.from,
                    Some(outgoing(switch.direction, pose.t)),
                    theme,
                    keyboard,
                    window,
                    cx,
                ) {
                    surface = surface.child(page);
                }
                if let Some(page) = self.panel_page(
                    displayed,
                    Some(incoming(switch.direction, pose.t)),
                    theme,
                    keyboard,
                    window,
                    cx,
                ) {
                    surface = surface.child(page);
                }
            }
            _ => {
                if let Some(page) = self.panel_page(displayed, None, theme, keyboard, window, cx) {
                    surface = surface.child(page);
                }
            }
        }
        // The hairline paints above the pages so headers and scrolled rows never cover it.
        Some(surface.child(ring(theme, 0.1).rounded(px(16.0))))
    }

    fn panel_page(
        &mut self,
        id: PanelId,
        slide: Option<(f32, f32)>,
        theme: Theme,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let root = self.measures.root.get();
        let window_width = f32::from(window.viewport_size().width);
        let window_height = if root.size.height > px(0.0) {
            f32::from(root.size.height)
        } else {
            f32::from(window.viewport_size().height)
        };
        let (title, width, body, sticky): (&str, f32, AnyElement, Option<Div>) = match id {
            PanelId::Settings => (
                "Playback settings",
                288.0f32.min(window_width - 16.0),
                self.settings_body(theme, keyboard, window, cx),
                None,
            ),
            _ => {
                let content = self.panel_content(id)?;
                let width = content.width.min(window_width - 16.0);
                let highlight = self.panels.row.filter(|_| {
                    self.panels.active() == Some(id)
                        && self.focus.panel.contains_focused(window, cx)
                });
                if id == PanelId::Subtitles {
                    let shown = width.min(window_width - 40.0);
                    let body = self
                        .subtitles_body(&content, shown, highlight, theme, keyboard, window, cx);
                    (content.title, width, body, None)
                } else {
                    let (list, sticky) = self.list_body(&content, id, highlight, theme, cx);
                    (content.title, width, list.into_any_element(), sticky)
                }
            }
        };
        let (x, opacity) = slide.map_or((0.0, 1.0), |(x, opacity)| (x * width, opacity));
        let page = div()
            .id(("player-panel-page", id as usize))
            .role(gpui::Role::Dialog)
            .aria_label(title)
            .relative()
            .w(px(width))
            .max_w(px(window_width - 40.0))
            .max_h(px(448.0f32.min(window_height - 160.0)))
            .flex()
            .flex_col()
            .overflow_hidden()
            .opacity(opacity)
            .when(slide.is_some(), |page| page.absolute().top_0().left(px(x)))
            .child(probe(&self.measures, move |measures, bounds| {
                measures.pages[id as usize].set(Some(bounds.size));
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(44.0))
                    .px(px(20.0))
                    .flex()
                    .items_center()
                    .bg(theme.player_canvas)
                    .label()
                    .child(title),
            )
            .child(body);
        Some(match sticky {
            Some(overlay) => page.child(overlay),
            None => page,
        })
    }

    /// The scrolling list of the Audio and Sources panels, and its sticky section label.
    fn list_body(
        &mut self,
        content: &Content,
        id: PanelId,
        highlight: Option<usize>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> (Stateful<Div>, Option<Div>) {
        let items = content.items();
        let scroll = self.scrolls[id as usize].clone();
        let mut label_tops = vec![];
        let mut list = div()
            .id(("player-panel-list", id as usize))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
            .flex()
            .flex_col()
            .px(px(8.0))
            .pt(px(4.0))
            .pb(px(8.0));
        for (index, item) in items.iter().enumerate() {
            let element = match *item {
                Item::Label(section) => {
                    if let Some(bounds) = scroll.bounds_for_item(index) {
                        label_tops.push(f32::from(bounds.top()));
                    }
                    section_label(
                        content.sections[section].label.as_deref().unwrap_or(""),
                        theme,
                    )
                    .into_any_element()
                }
                Item::Row(flat) => match content.row(flat) {
                    Some(row) => self
                        .option_row(row, flat, highlight == Some(flat), theme, cx)
                        .into_any_element(),
                    None => continue,
                },
                Item::Note(section) => match &content.sections[section].note {
                    Some(note) => caption(&note.text, theme, note.danger)
                        .px(px(12.0))
                        .py(px(8.0))
                        .into_any_element(),
                    None => continue,
                },
                Item::Empty => caption(content.empty.unwrap_or(""), theme, false)
                    .px(px(12.0))
                    .pb(px(8.0))
                    .into_any_element(),
                Item::Error => caption(content.error.as_deref().unwrap_or(""), theme, true)
                    .px(px(12.0))
                    .pt(px(8.0))
                    .pb(px(8.0))
                    .into_any_element(),
            };
            list = list.child(div().flex_shrink_0().child(element));
        }
        let sticky = sticky_overlay(&scroll, &label_tops, content, theme);
        (list, sticky)
    }

    pub(super) fn option_row(
        &self,
        row: &Row,
        flat: usize,
        highlighted: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let ink = theme.ink;
        let clamp = |text: Div| {
            if row.multiline {
                text.line_clamp(2)
            } else {
                text.truncate()
            }
        };
        let key = row.key;
        div()
            .id(("player-row", flat))
            .role(gpui::Role::RadioButton)
            .aria_label(row.label.clone())
            .relative()
            .flex()
            .items_center()
            .gap(px(12.0))
            .min_h(px(44.0))
            .rounded(px(8.0))
            .px(px(12.0))
            .py(px(8.0))
            .when(row.selected, |element| element.bg(ink.opacity(0.1)))
            .when(!row.selected, |element| {
                element.hover(move |style| style.bg(ink.opacity(0.05)))
            })
            .when(row.disabled, |element| {
                element.opacity(0.5).cursor_not_allowed()
            })
            .when(!row.disabled, |element| {
                element
                    .cursor_pointer()
                    .on_click(cx.listener(move |player, _, window, cx| {
                        player.select_row(key, window, cx);
                    }))
            })
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(clamp(
                        div()
                            .text_size(px(14.0))
                            .line_height(relative(1.43))
                            .child(row.label.clone()),
                    ))
                    .when_some(row.description.clone(), |column, description| {
                        column.child(clamp(
                            div()
                                .mt(px(2.0))
                                .caption()
                                .text_color(ink.opacity(0.55))
                                .child(description),
                        ))
                    }),
            )
            .when(!row.tags.is_empty(), |element| {
                element.child(div().flex().flex_shrink_0().gap(px(4.0)).children(
                    row.tags.iter().map(|tag| {
                        div()
                            .caption()
                            .font_features(tabular())
                            .rounded(px(4.0))
                            .px(px(6.0))
                            .bg(ink.opacity(0.1))
                            .text_color(ink.opacity(0.8))
                            .child(tag.clone())
                    }),
                ))
            })
            .when(highlighted, |element| {
                element.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(8.0))
                        .border_2()
                        .border_color(ink),
                )
            })
    }
}

pub(super) fn caption(text: &str, theme: Theme, danger: bool) -> Div {
    div()
        .caption()
        .text_color(if danger {
            theme.danger
        } else {
            theme.ink.opacity(0.55)
        })
        .child(text.to_owned())
}

fn section_label(text: &str, theme: Theme) -> Div {
    caption(text, theme, false)
        .px(px(12.0))
        .pt(px(12.0))
        .pb(px(4.0))
}

fn sticky_overlay(
    scroll: &ScrollHandle,
    tops: &[f32],
    content: &Content,
    theme: Theme,
) -> Option<Div> {
    let labelled: Vec<usize> = content
        .sections
        .iter()
        .enumerate()
        .filter(|(_, section)| section.label.is_some())
        .map(|(index, _)| index)
        .collect();
    if tops.len() != labelled.len() {
        return None;
    }
    let view_top = f32::from(scroll.bounds().top());
    let (active, y) = sticky_label(view_top, tops)?;
    let text = content.sections[labelled[active]].label.as_deref()?;
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(44.0 + y))
            .px(px(8.0))
            .bg(theme.player_canvas)
            .child(section_label(text, theme)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_stick_only_after_scrolling_past_the_stick_line() {
        assert_eq!(sticky_label(100.0, &[104.0, 300.0]), None);
        assert_eq!(sticky_label(100.0, &[91.0, 300.0]), Some((0, -8.0)));
    }

    #[test]
    fn the_next_label_pushes_the_stuck_one_out() {
        let pushed = sticky_label(100.0, &[10.0, 110.0]).expect("stuck");
        assert_eq!(pushed.0, 0);
        assert!((pushed.1 - (110.0 - LABEL_HEIGHT - 100.0)).abs() < 1e-4);
        assert_eq!(sticky_label(100.0, &[10.0, 90.0, 400.0]), Some((1, -8.0)));
    }
}
