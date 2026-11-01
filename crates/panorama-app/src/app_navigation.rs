use super::*;
use crate::zoom::{CardArt, Direction, Quiet, Returning, Zoom};
use panorama_core::addons::FilmDetails;

/// Whether a key is someone moving through the controls, the only time the focus ring is wanted.
/// A click leaves its card focused, so Backspace or typing must not ring it.
pub(super) fn moves_focus(keystroke: &gpui::Keystroke) -> bool {
    let modifiers = keystroke.modifiers;
    !modifiers.platform
        && !modifiers.alt
        && !modifiers.control
        && matches!(
            keystroke.key.as_str(),
            "tab" | "up" | "down" | "left" | "right"
        )
}

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

    pub(crate) fn focus_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar
            .focus(self.history.current().destination(), window, cx);
    }

    fn current_view<T: 'static>(&self) -> Option<Entity<T>> {
        let id = self.history.current_id();
        self.cache
            .iter()
            .find(|entry| entry.id == id)
            .or(self.mounted.as_ref().filter(|entry| entry.id == id))
            .and_then(|entry| entry.view.clone().downcast::<T>().ok())
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search) = self.current_view::<routes::search::Search>() {
            search.update(cx, |search, cx| search.focus_field(window, cx));
        }
    }

    /// Push a route while preserving existing view instances for backward navigation.
    /// Grow `card` to fill the window, then open its film. Reduced motion goes straight there.
    pub fn zoom_to_film(
        &mut self,
        film: FilmDetails,
        card: CardArt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.reduced_motion {
            return self.open_film(film, window, cx);
        }
        self.remember(&card, &film, window);
        let open = self.open_when_landed(film.clone(), window, cx);
        self.zoom = Some(Zoom::new(card, film, Direction::Open, open));
        cx.notify();
    }
    /// A click on the artwork on its way back turns it around, onto the same film.
    pub fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(zoom) = self.zoom.as_ref() else {
            return;
        };
        let (card, film) = (zoom.card.clone(), zoom.film.clone());
        self.state.update(cx, |state, _| state.quiet = None);
        self.remember(&card, &film, window);
        let open = self.open_when_landed(film, window, cx);
        if let Some(zoom) = self.zoom.as_mut() {
            zoom.turn(Direction::Open, open);
        }
        cx.notify();
    }
    fn remember(&mut self, card: &CardArt, film: &FilmDetails, window: &Window) {
        self.returning = Some(Returning {
            card: card.clone(),
            film: film.clone(),
            from: self.history.current().clone(),
            viewport: window.viewport_size(),
        });
    }
    fn open_when_landed(
        &self,
        film: FilmDetails,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn_in(window, async move |shell, cx| {
            cx.background_executor().timer(crate::zoom::DURATION).await;
            let _ = shell.update_in(cx, |shell, window, cx| shell.open_film(film, window, cx));
        })
    }
    fn clear_when_landed(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(crate::zoom::DURATION).await;
            let _ = shell.update(cx, |shell, cx| {
                shell.zoom = None;
                cx.notify();
            });
        })
    }
    /// Shrink the page's banner onto the card it grew from, over the page just returned to.
    fn zoom_back(&mut self, returning: Returning, cx: &mut Context<Self>) {
        let Returning { mut card, film, .. } = returning;
        let url = if card.poster {
            card.film.poster.as_ref()
        } else {
            card.film.background.as_ref()
        };
        let images = self.state.read(cx).images.read(cx);
        if let Some(sharp) = url.and_then(|url| images.newest(url.as_str())) {
            card.image = sharp;
        }
        let quiet = Quiet::new(card.film.id.clone());
        self.state.update(cx, |state, _| state.quiet = Some(quiet));
        let done = self.clear_when_landed(cx);
        self.zoom = Some(Zoom::new(card, film, Direction::Close, done));
        cx.notify();
    }
    fn open_film(&mut self, film: FilmDetails, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom = None;
        let (id, kind) = (film.id.clone(), film.kind);
        self.state
            .update(cx, |state, _| state.film_preview = Some(film));
        self.navigate(Route::Film { id, kind }, window, cx);
    }
    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.push(route) {
            self.changed(window, cx);
        }
    }

    /// Swap the active entry for `route`, leaving no history entry behind.
    pub fn redirect(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.replace(route) {
            self.changed(window, cx);
        }
    }

    pub fn back_to_film(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.state.read(cx);
        let kind = (state.playback.as_ref().map(|playing| &playing.film))
            .or(state.launch.as_ref().map(|launch| &launch.film))
            .filter(|film| film.id == id)
            .map_or(Default::default(), |film| film.kind);
        if self.history.leave_for(Route::Film { id, kind }) {
            self.changed(window, cx);
        }
    }

    /// Go backward when history permits it.
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Mid-zoom, Back belongs to the zoom: the film page has not opened, or has already gone.
        if let Some(direction) = self.zoom.as_ref().map(|zoom| zoom.direction) {
            if direction == Direction::Open {
                self.returning = None;
                let done = self.clear_when_landed(cx);
                if let Some(zoom) = self.zoom.as_mut() {
                    let quiet = Quiet::new(zoom.card.film.id.clone());
                    zoom.turn(Direction::Close, done);
                    self.state.update(cx, |state, _| state.quiet = Some(quiet));
                }
                cx.notify();
            }
            return;
        }
        let returning = match self.history.current() {
            Route::Film { id, .. } => self
                .returning
                .take()
                .filter(|r| r.film.id == *id && r.viewport == window.viewport_size()),
            _ => None,
        };
        if self.history.can_back() && self.history.back() {
            self.changed(window, cx);
            if let Some(returning) = returning.filter(|r| r.from == *self.history.current()) {
                self.zoom_back(returning, cx);
            }
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
        self.zoom = None;
        window.focus(&self.focus, cx);
        self.present(window, cx);
        cx.notify();
    }

    pub(super) fn present(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.history.current_id();
        let existing = self
            .cache
            .iter()
            .position(|entry| entry.id == id)
            .and_then(|index| self.cache.remove(index))
            .or_else(|| self.mounted.clone().filter(|entry| entry.id == id));
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
        self.cache.retain(|entry| {
            self.history.contains(entry.id) && !matches!(entry.route, Route::Player { .. })
        });
        if !matches!(mounted.route, Route::Player { .. }) {
            self.cache.push_back(mounted.clone());
        }
        while self.cache.len() > 8 {
            self.cache.pop_front();
        }
        self.mounted = Some(mounted);
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

#[cfg(test)]
mod tests {
    use super::moves_focus;

    #[test]
    fn only_navigation_keys_show_the_focus_ring() {
        let moves = |key| moves_focus(&gpui::Keystroke::parse(key).unwrap());
        for key in ["tab", "shift-tab", "up", "down", "left", "right"] {
            assert!(moves(key), "{key}");
        }
        for key in [
            "backspace",
            "enter",
            "escape",
            "a",
            "alt-left",
            "cmd-shift-4",
        ] {
            assert!(!moves(key), "{key}");
        }
    }
}
