use super::*;

impl AppShell {
    /// Push a route while preserving existing view instances for backward navigation.
    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.push(route) {
            self.changed(window, cx);
        }
    }

    /// Go backward when history permits it.
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.can_back() && self.history.back() {
            self.changed(window, cx);
        }
    }

    /// Go forward when history permits it.
    pub fn forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.can_forward() && self.history.forward() {
            self.changed(window, cx);
        }
    }

    /// Whether an entry owns the active route controls.
    pub fn is_current(&self, id: u64) -> bool {
        self.history.current_id() == id
    }

    pub(super) fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu.dismiss();
        let open = self
            .search_disclosures
            .get(&self.history.current_id())
            .copied()
            .unwrap_or(matches!(self.history.current(), Route::Search { .. }));
        self.search.set_open(open);
        if let Route::Search { query } = self.history.current() {
            self.search.query.clone_from(query);
            self.search_input
                .update(cx, |input, cx| input.set_value(query.clone(), window, cx));
        }
        self.search_disclosures
            .retain(|id, _| self.history.contains(*id));
        self.search_caret(cx);
        self.sticky = if matches!(self.history.current(), Route::Home | Route::Film { .. }) {
            Sticky::home()
        } else {
            Sticky::default()
        };
        self.header_slide = Tween::fixed(0.0);
        window.focus(&self.focus, cx);
        self.present(cx);
        cx.notify();
    }

    pub(super) fn present(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let id = self.history.current_id();
        let existing = self
            .cache
            .iter()
            .position(|entry| entry.id == id)
            .and_then(|index| self.cache.remove(index))
            .or_else(|| {
                self.presence
                    .iter()
                    .find(|presence| presence.view.id == id)
                    .map(|presence| presence.view.clone())
            });
        let mounted = existing.unwrap_or_else(|| {
            let route = self.history.current().clone();
            Rc::new(Mounted {
                id,
                header: Header::new(cx),
                view: routes::create(
                    &route,
                    id,
                    cx.weak_entity(),
                    self.state.clone(),
                    self.args.clone(),
                    cx,
                ),
                route,
            })
        });
        self.cache.retain(|entry| self.history.contains(entry.id));
        self.cache.push_back(mounted.clone());
        while self.cache.len() > 8 {
            self.cache.pop_front();
        }
        if self.reduced_motion {
            self.presence.clear();
        }
        for presence in &mut self.presence {
            if presence.view.id == id {
                presence.resume(now);
            } else {
                presence.exit(now);
            }
        }
        if !self.presence.iter().any(|presence| presence.view.id == id) {
            self.presence.push(Presence::enter(mounted, now));
        }
        self.presence.sort_by_key(|presence| !presence.exiting);
    }

    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.login.is_some() {
            return;
        }
        let key = event.keystroke.key.as_str();
        let modifiers = &event.keystroke.modifiers;
        let input = window.has_focused_input(cx);
        match key {
            "escape"
                if self.search.open || matches!(self.history.current(), Route::Search { .. }) =>
            {
                self.clear_search(window, cx);
            }
            "escape" if self.menu.open => {
                self.menu.set_open(false);
                if let Some(entry) = self
                    .cache
                    .iter()
                    .find(|entry| entry.id == self.history.current_id())
                {
                    entry.header.account.focus(window, cx);
                }
                cx.notify();
            }
            "left" if modifiers.alt => self.back(window, cx),
            "right" if modifiers.alt => self.forward(window, cx),
            "backspace"
                if !input && !modifiers.control && !modifiers.alt && !modifiers.platform =>
            {
                self.back(window, cx)
            }
            "k" if modifiers.control => self.toggle_search(window, cx),
            "/" if !input && !modifiers.control && !modifiers.alt && !modifiers.platform => {
                self.toggle_search(window, cx)
            }
            "escape" if matches!(self.history.current(), Route::Player { .. }) => {
                self.back(window, cx)
            }
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }
}
