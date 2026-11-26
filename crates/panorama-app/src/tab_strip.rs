//! A row of text tabs, as MUBI sets a show's seasons: festival editions, a show's seasons.
use crate::{
    home_layout::ViewSettings,
    theme::{DURATION_STANDARD, EASE_EDITORIAL, Theme, caps, focus_ring, typography::SECTION},
    transition::bezier,
};
use gpui::{
    Animation, AnimationExt, Div, ElementId, FocusHandle, Stateful, Window, canvas, div,
    prelude::*, px,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// One tab: its label, the space under it, and its bar.
const ROW: f32 = 44.0;
/// Between labels.
const GAP: f32 = 36.0;
/// The selected tab's bar, which sits on the strip's hairline.
const BAR: f32 = 3.0;

/// Where a strip's tabs sat last frame, so the bar can slide from the old tab to a new one.
#[derive(Default)]
pub struct Slide {
    /// Each label's left edge in the window and its width, by tab.
    spans: Rc<RefCell<Vec<(f32, f32)>>>,
    shown: Cell<Option<usize>>,
    /// The span the bar is sliding from, kept until the next pick.
    from: Cell<Option<(f32, f32)>>,
}

impl Slide {
    /// Tab `index` is selected this frame: where its bar slides from, if it just was picked.
    fn select(&self, index: usize) -> Option<(f32, f32)> {
        if self.shown.get() != Some(index) {
            let previous = self.shown.get();
            self.from
                .set(previous.and_then(|i| self.spans.borrow().get(i).copied()));
            self.shown.set(Some(index));
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
                    .pb(px(12.0 + BAR))
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

/// The tabs in a row across `width`, over a hairline the selected tab's bar rests on.
pub fn strip(
    id: &'static str,
    label: &'static str,
    width: f32,
    theme: Theme,
    tabs: impl IntoIterator<Item = Stateful<Div>>,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(gpui::Role::TabList)
        .aria_label(label)
        .w(px(width))
        // A scroll container takes no height from its row; this is one tab.
        .h(px(ROW + 1.0))
        .overflow_x_scroll()
        .border_b_1()
        .border_color(theme.rule)
        .flex()
        .gap(px(GAP))
        .children(tabs)
}

#[cfg(test)]
mod tests {
    use super::Slide;

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
