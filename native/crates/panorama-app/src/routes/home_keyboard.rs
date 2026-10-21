use super::*;

impl Home {
    /// Move keyboard focus from WATCH into the visible first row.
    pub(crate) fn focus_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.switcher_visible(cx) {
            self.focus_switcher(window, cx);
        } else {
            self.focus_first_card(window, cx);
        }
    }
    /// Focus the first card, scrolled into view.
    pub(crate) fn focus_first_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lead = self.lead(cx);
        if let Some(first) = self.grid.focus(0) {
            let grid = crate::home_layout::Grid::fit(self.settings(window, cx).column().width);
            self.scroll
                .set_offset(point(px(0.0), px(-(lead + grid.height()))));
            first.focus(window, cx);
            self.tick(window, cx);
            cx.notify();
        }
        cx.stop_propagation();
    }
    /// Wrap catalog keyboard navigation to the visible header.
    pub(crate) fn focus_top(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scroll.set_offset(point(px(0.0), px(0.0)));
        self.tick(window, cx);
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.focus_sidebar(window, cx));
        cx.stop_propagation();
        cx.notify();
    }
    /// Keep focused cards visible while navigating virtual rows.
    pub(crate) fn step_card(
        &mut self,
        index: usize,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.grid.is_empty() {
            cx.stop_propagation();
            return;
        }
        let index = index.min(self.grid.len());
        let view = self.settings(window, cx);
        let grid = crate::home_layout::Grid::fit(view.column().width);
        if !forward && index == 0 {
            if self.switcher_visible(cx) {
                self.focus_switcher(window, cx);
                return;
            }
            self.watch.focus(window, cx);
            self.scroll.set_offset(point(px(0.0), px(0.0)));
        } else if forward && index + 1 >= self.grid.len() {
            if !self
                .state
                .read(cx)
                .catalog
                .as_ref()
                .is_some_and(|page| page.has_more)
            {
                self.scroll.set_offset(point(px(0.0), px(0.0)));
                let _ = self
                    .shell
                    .update(cx, |shell, cx| shell.focus_sidebar(window, cx));
            } else {
                self.more.focus(window, cx);
                self.scroll.set_offset(point(
                    px(0.0),
                    px(-(view.height
                        + self.lead(cx)
                        + self.grid.len().div_ceil(grid.columns) as f32 * grid.stride()
                        + 64.0
                        - view.height
                        + 40.0)),
                ));
            }
        } else {
            let next = if forward { index + 1 } else { index - 1 };
            let y = view.height + self.lead(cx) + FilmGrid::row_top(next, view.column());
            crate::film_grid::reveal(&self.scroll, y, grid.height(), view.height);
            if let Some(card) = self.grid.focus(next) {
                card.focus(window, cx);
            }
        }
        cx.stop_propagation();
        cx.notify();
        self.tick(window, cx);
    }
}
