use super::*;

impl Home {
    pub(crate) fn focus_below_hero(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.row.cards.is_empty() {
            self.focus_first(window, cx);
        } else {
            self.focus_row(0, window, cx);
        }
        cx.stop_propagation();
    }
    pub(crate) fn focus_above_catalog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(last) = self.row.cards.len().checked_sub(1) {
            self.focus_row(last, window, cx);
        } else {
            let signed_in = self.signed_in(cx);
            self.hero.last(signed_in).focus(window, cx);
            self.scroll.set_offset(point(px(0.0), px(0.0)));
        }
        self.tick(window, cx);
        cx.stop_propagation();
        cx.notify();
    }
    fn focus_row(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.row.cards.get(index) else {
            return;
        };
        card.focus(window, cx);
        let view = self.settings(window, cx);
        self.row.show(index, view);
        let height = crate::continue_row::card_size(view).1;
        crate::film_grid::reveal(
            &self.scroll,
            self.fold - crate::hero::FOLD_PAD - height,
            height,
            view.height,
        );
        cx.notify();
    }
    pub(crate) fn step_row(
        &mut self,
        index: usize,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (forward, index) {
            (false, 0) => {
                let signed_in = self.signed_in(cx);
                self.hero.last(signed_in).focus(window, cx);
            }
            (false, _) => self.focus_row(index - 1, window, cx),
            (true, _) if index + 1 < self.row.cards.len() => self.focus_row(index + 1, window, cx),
            (true, _) => self.focus_first(window, cx),
        }
        self.tick(window, cx);
        cx.stop_propagation();
    }
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
            self.scroll.set_offset(point(
                px(0.0),
                px(-(self.fold - self.settings(window, cx).height + lead + grid.height())),
            ));
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
            self.focus_above_catalog(window, cx);
            return;
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
                    px(-(self.fold
                        + self.lead(cx)
                        + self.grid.len().div_ceil(grid.columns) as f32 * grid.stride()
                        + 64.0
                        - view.height
                        + 40.0)),
                ));
            }
        } else {
            let next = if forward { index + 1 } else { index - 1 };
            let y = self.fold + self.lead(cx) + FilmGrid::row_top(next, view.column());
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
