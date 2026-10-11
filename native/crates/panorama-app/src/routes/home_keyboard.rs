use super::*;

impl Home {
    /// Move keyboard focus from WATCH into the visible first row.
    pub(crate) fn focus_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(first) = self.cards.first() {
            let grid = crate::home_layout::Grid::at(self.settings(window, cx).width);
            self.scroll
                .set_offset(point(px(0.0), px(-(32.0 + grid.height()))));
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
            .update(cx, |shell, cx| shell.focus_header(window, cx));
        cx.stop_propagation();
        cx.notify();
    }
    /// Keep focused cards visible while navigating virtual rows.
    pub(crate) fn focus_card(
        &mut self,
        index: usize,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.cards.is_empty() {
            cx.stop_propagation();
            return;
        }
        let index = index.min(self.cards.len());
        let view = self.settings(window, cx);
        let grid = crate::home_layout::Grid::at(view.width);
        if !forward && index == 0 {
            self.watch.focus(window, cx);
            self.scroll.set_offset(point(px(0.0), px(0.0)));
        } else if forward && index + 1 >= self.cards.len() {
            if !self
                .state
                .read(cx)
                .catalog
                .as_ref()
                .is_some_and(|page| page.has_more)
                || self.search.is_some()
            {
                self.scroll.set_offset(point(px(0.0), px(0.0)));
                let _ = self
                    .shell
                    .update(cx, |shell, cx| shell.focus_header(window, cx));
            } else {
                self.more.focus(window, cx);
                self.scroll.set_offset(point(
                    px(0.0),
                    px(-(view.height
                        + 32.0
                        + self.films.len().div_ceil(grid.columns) as f32 * grid.stride()
                        + 64.0
                        - view.height
                        + 40.0)),
                ));
            }
        } else {
            let next = if forward { index + 1 } else { index - 1 };
            let y = self.content_start(view, cx)
                + if self.search.is_some() { 72.0 } else { 0.0 }
                + 32.0
                + (next / grid.columns) as f32 * grid.stride();
            let top = -f32::from(self.scroll.offset().y);
            if y < top {
                self.scroll.set_offset(point(px(0.0), px(-y)));
            } else if y + grid.height() > top + view.height {
                self.scroll
                    .set_offset(point(px(0.0), px(-(y + grid.height() - view.height))));
            }
            self.cards[next].focus(window, cx);
        }
        cx.stop_propagation();
        cx.notify();
        self.tick(window, cx);
    }
}
