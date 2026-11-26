//! A row of text tabs, as MUBI sets a show's seasons: festival editions, a show's seasons.
use crate::{
    home_layout::ViewSettings,
    theme::{
        DURATION_DELIBERATE, DURATION_STANDARD, EASE_EDITORIAL, caps, focus_ring,
        typography::SECTION,
    },
    transition::{Tween, bezier},
};
use gpui::{
    Animation, AnimationExt, Div, ElementId, FocusHandle, ScrollHandle, ScrollWheelEvent, Stateful,
    Window, canvas, div, point, prelude::*, px, svg,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};

/// One tab: its label, the space under it, and its bar.
const ROW: f32 = 44.0;
/// Between labels.
const GAP: f32 = 36.0;
/// The selected tab's bar, which sits on the strip's hairline.
const BAR: f32 = 3.0;
/// The buttons that page a row longer than the column.
const PAGER: f32 = 28.0;
/// Between the column's edge and a paging button in the margin.
const PAGER_GAP: f32 = 16.0;
/// Under a label: the space down to its bar, then the bar.
const UNDER: f32 = 12.0 + BAR;
/// Where a label's line of text centres down a tab, so the paging buttons sit level with it.
const LABEL_MIDDLE: f32 = ROW - UNDER - (SECTION.size + 2.0) * SECTION.line_height / 2.0;

/// Where a strip's tabs sat last frame, so the bar can slide from the old tab to a new one.
#[derive(Default)]
pub struct Slide {
    /// Each label's left edge in the window and its width, by tab.
    spans: Rc<RefCell<Vec<(f32, f32)>>>,
    shown: Cell<Option<usize>>,
    /// The span the bar is sliding from, kept until the next pick.
    from: Cell<Option<(f32, f32)>>,
    /// The strip's sideways scroll, for a row longer than the column.
    scroll: ScrollHandle,
    /// A page the buttons asked for, gliding there as the carousels do.
    glide: Rc<Cell<Option<Tween>>>,
    /// A picked tab waiting to be brought into view once the row has laid out and overflows.
    reveal: Cell<bool>,
}

impl Slide {
    /// Tab `index` is selected this frame: where its bar slides from, if it just was picked.
    fn select(&self, index: usize) -> Option<(f32, f32)> {
        if self.shown.get() != Some(index) {
            let previous = self.shown.get();
            self.from
                .set(previous.and_then(|i| self.spans.borrow().get(i).copied()));
            self.shown.set(Some(index));
            // A picked tab, or the one a page opens on, is brought into view, and no paging
            // glide may carry it back out.
            self.reveal.set(true);
            self.glide.set(None);
        }
        self.from.get()
    }
}

/// A tab, its label set in capitals like a section heading.
#[allow(clippy::too_many_arguments)]
pub fn tab(
    id: impl Into<ElementId>,
    index: usize,
    label: &str,
    selected: bool,
    slide: &Slide,
    focus: &FocusHandle,
    view: ViewSettings,
    window: &Window,
) -> Stateful<Div> {
    let theme = view.theme;
    let spans = slide.spans.clone();
    let measure = canvas(
        move |bounds, _, _| {
            let mut spans = spans.borrow_mut();
            if spans.len() <= index {
                spans.resize(index + 1, (0.0, 0.0));
            }
            spans[index] = (f32::from(bounds.origin.x), f32::from(bounds.size.width));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0();
    let bar = selected.then(|| {
        let bar = div()
            .absolute()
            .bottom_0()
            .left_0()
            .h(px(BAR))
            .w_full()
            .bg(theme.ink);
        let here = slide.spans.borrow().get(index).copied();
        match (slide.select(index), here) {
            // From the old tab's place and width to this one's, as one bar moving along.
            (Some((from_x, from_w)), Some((x, w))) if !view.reduced && w > 0.0 => bar
                .with_animation(
                    ("tab-bar", index),
                    Animation::new(DURATION_STANDARD).with_easing(|t| bezier(t, EASE_EDITORIAL)),
                    move |bar, t| {
                        let left = (from_x - x) * (1.0 - t);
                        bar.left(px(left)).w(px(from_w + (w - from_w) * t))
                    },
                )
                .into_any_element(),
            _ => bar.into_any_element(),
        }
    });
    focus_ring(
        div()
            .id(id)
            .role(gpui::Role::Tab)
            .aria_label(label.to_owned())
            .cursor_pointer()
            .flex_none()
            .relative()
            .h(px(ROW))
            .flex()
            .flex_col()
            .justify_end()
            .text_color(if selected { theme.ink } else { theme.ink_muted })
            .hover(|style| style.text_color(theme.ink))
            .child(
                SECTION
                    .apply(div())
                    .text_size(px(SECTION.size + 2.0))
                    .relative()
                    .pb(px(UNDER))
                    .child(caps(label))
                    .child(measure),
            )
            .children(bar),
        focus,
        theme,
        view.active,
        view.keyboard,
        0.0,
        window,
    )
}

/// The tabs in a row across `width`, over a hairline the selected tab's bar rests on. A row
/// longer than the column scrolls sideways under a sideways gesture, and a button in the margin
/// beside each clipped end pages it for a mouse with only a vertical wheel.
#[allow(clippy::too_many_arguments)]
pub fn strip(
    id: &'static str,
    label: &'static str,
    width: f32,
    view: ViewSettings,
    slide: &Slide,
    window: &Window,
    tabs: impl IntoIterator<Item = Stateful<Div>>,
) -> Div {
    let theme = view.theme;
    let scroll = slide.scroll.clone();
    let glide = slide.glide.clone();
    // GPUI drops a reveal asked for before the row has laid out, so the strip places it itself
    // once it knows where the tabs sat, and only then lets the request go.
    if slide.reveal.get()
        && let Some(index) = slide.shown.get()
        && let Some(offset) = revealing(&slide.spans.borrow(), index, width, &scroll)
    {
        scroll.set_offset(point(px(offset), scroll.offset().y));
        slide.reveal.set(false);
        window.request_animation_frame();
    }
    if let Some(tween) = glide.get() {
        let now = Instant::now();
        scroll.set_offset(point(
            px(-tween.value(now, view.reduced)),
            scroll.offset().y,
        ));
        if tween.moving(now, view.reduced) {
            window.request_animation_frame();
        } else {
            glide.set(None);
        }
    }
    let max = f32::from(scroll.max_offset().x).max(0.0);
    let position = crate::carousel::clamped(scroll.offset().x, max);
    let held = glide.clone();
    let row = div()
        .id(id)
        .role(gpui::Role::TabList)
        .aria_label(label)
        .w(px(width))
        // A scroll container takes no height from its row; this is one tab.
        .h(px(ROW))
        .overflow_x_scroll()
        .track_scroll(&scroll)
        .flex()
        .gap(px(GAP))
        // Only sideways gestures move the row, so scrolling down over it moves the page.
        .restrict_scroll_to_axis()
        .on_scroll_wheel(move |_: &ScrollWheelEvent, window, _| {
            // A swipe takes over from a glide under way.
            held.set(None);
            window.refresh();
        })
        .children(tabs)
        // The buttons follow the laid-out row, which can change without a render: after a
        // resize, or on the first frame, before the row knows its length.
        .child({
            let (scroll, shown) = (scroll.clone(), (position > 1.0, position < max - 1.0));
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let max = f32::from(scroll.max_offset().x).max(0.0);
                    let position = crate::carousel::clamped(scroll.offset().x, max);
                    if (position > 1.0, position < max - 1.0) != shown {
                        window.request_animation_frame();
                    }
                },
            )
            .absolute()
            .size_0()
        });
    let pager = |forward: bool| {
        let (scroll, glide) = (scroll.clone(), glide.clone());
        div()
            .id((id, usize::from(forward)))
            .flex_none()
            .size(px(PAGER))
            .rounded_full()
            .bg(theme.arrow)
            .border_1()
            .border_color(theme.arrow_rim)
            .hover(|style| style.bg(theme.arrow_hover))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .role(gpui::Role::Button)
            .aria_label(if forward {
                "Later tabs"
            } else {
                "Earlier tabs"
            })
            .on_click(move |_, window, _| {
                let max = f32::from(scroll.max_offset().x).max(0.0);
                let from = crate::carousel::clamped(scroll.offset().x, max);
                let step = width * 0.7 * if forward { 1.0 } else { -1.0 };
                let now = Instant::now();
                let mut tween = Tween::fixed(from);
                tween.retarget(
                    (from + step).clamp(0.0, max),
                    DURATION_DELIBERATE,
                    Some(EASE_EDITORIAL),
                    now,
                );
                glide.set(Some(tween));
                window.refresh();
            })
            .child(
                svg()
                    .path(if forward {
                        "chevron-right.svg"
                    } else {
                        "chevron-left.svg"
                    })
                    .size(px(16.0))
                    .text_color(theme.ink),
            )
    };
    // The buttons sit just outside the column, in its margins, so the row keeps the column's
    // exact width and left edge whether or not they show, and a reveal lands where aimed.
    let pager_at = |forward: bool| {
        div()
            .absolute()
            .top(px(LABEL_MIDDLE - PAGER / 2.0))
            .map(|slot| {
                if forward {
                    slot.left(px(width + PAGER_GAP))
                } else {
                    slot.left(px(-PAGER_GAP - PAGER))
                }
            })
            .child(pager(forward))
    };
    div()
        .relative()
        .w(px(width))
        .border_b_1()
        .border_color(theme.rule)
        .child(row)
        .when(position > 1.0, |strip| strip.child(pager_at(false)))
        .when(position < max - 1.0, |strip| strip.child(pager_at(true)))
}

/// The scroll offset that brings tab `index` wholly into a row `width` wide, from the labels'
/// window positions last frame; `None` until the row has laid out.
fn revealing(spans: &[(f32, f32)], index: usize, width: f32, scroll: &ScrollHandle) -> Option<f32> {
    let max = f32::from(scroll.max_offset().x);
    let (&(first, _), &(x, w)) = (spans.first()?, spans.get(index)?);
    if max <= 0.0 || w <= 0.0 {
        return None;
    }
    // Where the tab starts along the row, whatever the row has scrolled to.
    let along = x - first;
    let shown = -f32::from(scroll.offset().x);
    Some(-reveal(along, w, shown, width).clamp(0.0, max))
}

/// How far a row `width` wide should have scrolled to show a tab starting `along` it and `w`
/// wide, moving as little as it can from `shown`.
fn reveal(along: f32, w: f32, shown: f32, width: f32) -> f32 {
    if along < shown {
        along
    } else if along + w > shown + width {
        along + w - width
    } else {
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::Slide;

    #[test]
    fn a_reveal_moves_the_row_only_as_far_as_the_tab_needs() {
        use super::reveal;
        assert_eq!(
            reveal(1900.0, 60.0, 0.0, 1100.0),
            860.0,
            "off the right end"
        );
        assert_eq!(
            reveal(100.0, 60.0, 400.0, 1100.0),
            100.0,
            "off the left end"
        );
        assert_eq!(reveal(500.0, 60.0, 400.0, 1100.0), 400.0, "already in view");
    }

    #[test]
    fn the_bar_slides_from_the_tab_picked_before() {
        let slide = Slide::default();
        assert_eq!(
            slide.select(0),
            None,
            "the first tab shown has nowhere to come from"
        );
        *slide.spans.borrow_mut() = vec![(100.0, 80.0), (216.0, 90.0)];
        assert_eq!(slide.select(0), None, "staying put does not slide");
        assert_eq!(slide.select(1), Some((100.0, 80.0)));
        assert_eq!(
            slide.select(1),
            Some((100.0, 80.0)),
            "the slide holds through its frames"
        );
        assert_eq!(slide.select(0), Some((216.0, 90.0)));
    }
}
