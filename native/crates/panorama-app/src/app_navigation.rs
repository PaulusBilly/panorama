use super::*;

impl AppShell {
    /// Sidebar click, `/`, Ctrl+K. Selecting the current destination is a no-op, except Search,
    /// which refocuses its field. Other selections push (so Back returns).
    pub fn go(&mut self, to: Destination, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.go(to) {
            self.changed(window, cx);
        }
        if to == Destination::Search {
            self.focus_search(window, cx);
        }
    }

    /// Rewrite entry `id`'s route in place, keeping its mounted view; Search records its query.
    pub fn retitle(&mut self, id: u64, route: Route) {
        self.history.retitle(id, route);
    }

    /// Focus the sidebar item for the current destination.
    pub(crate) fn focus_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar
            .focus(self.history.current().destination(), window, cx);
    }

    /// The mounted view of the current entry, if it is a `T`.
    fn current_view<T: 'static>(&self) -> Option<Entity<T>> {
        let id = self.history.current_id();
        self.cache
            .iter()
            .find(|entry| entry.id == id)
            .or_else(|| {
                self.presence
                    .iter()
                    .map(|presence| &presence.view)
                    .find(|entry| entry.id == id)
            })
            .and_then(|entry| entry.view.clone().downcast::<T>().ok())
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search) = self.current_view::<routes::search::Search>() {
            search.update(cx, |search, cx| search.focus_field(window, cx));
        }
    }

    /// Push a route while preserving existing view instances for backward navigation.
    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.push(route) {
            self.changed(window, cx);
        }
    }

    pub fn back_to_film(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.replace(Route::Film { id }) {
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
        window.focus(&self.focus, cx);
        self.present(window, cx);
        cx.notify();
    }

    pub(super) fn present(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                view: routes::create(
                    &route,
                    id,
                    cx.weak_entity(),
                    self.state.clone(),
                    self.args.clone(),
                    window,
                    cx,
                ),
                route,
            })
        });
        let player_transition = matches!(mounted.route, Route::Player { .. })
            || self
                .presence
                .iter()
                .any(|presence| matches!(presence.view.route, Route::Player { .. }));
        self.cache.retain(|entry| {
            self.history.contains(entry.id) && !matches!(entry.route, Route::Player { .. })
        });
        if !matches!(mounted.route, Route::Player { .. }) {
            self.cache.push_back(mounted.clone());
        }
        while self.cache.len() > 8 {
            self.cache.pop_front();
        }
        if self.reduced_motion || player_transition {
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
        let sidebar = self.history.current().has_sidebar();
        match key {
            "escape" if self.menu.open => {
                self.menu.set_open(false);
                self.sidebar.account.focus(window, cx);
                cx.notify();
            }
            "left" if modifiers.alt => self.back(window, cx),
            "right" if modifiers.alt => self.forward(window, cx),
            "backspace"
                if !input && !modifiers.control && !modifiers.alt && !modifiers.platform =>
            {
                self.back(window, cx)
            }
            "k" if modifiers.control && sidebar => self.go(Destination::Search, window, cx),
            "/" if sidebar
                && !input
                && !modifiers.control
                && !modifiers.alt
                && !modifiers.platform =>
            {
                self.go(Destination::Search, window, cx)
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
