//! Pure navigation history and debug route syntax.
use crate::button::Icon;

/// A top-level place the sidebar can take the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    Search,
    Home,
    Addons,
}

impl Destination {
    /// The single ordered list: sidebar items, focus handles and key bindings derive from it.
    pub const ALL: [Self; 3] = [Self::Search, Self::Home, Self::Addons];
    /// Accessible name of the sidebar item.
    pub fn label(self) -> &'static str {
        match self {
            Self::Search => "Search",
            Self::Home => "Home",
            Self::Addons => "Addons",
        }
    }
    /// Sidebar glyph.
    pub fn icon(self) -> Icon {
        match self {
            Self::Search => Icon::Search,
            Self::Home => Icon::Home,
            Self::Addons => Icon::Addons,
        }
    }
    /// The route a fresh visit opens; Search starts empty.
    pub fn route(self) -> Route {
        match self {
            Self::Search => Route::Search {
                query: String::new(),
            },
            Self::Home => Route::Home,
            Self::Addons => Route::Addons,
        }
    }
}

/// A native destination; identifiers remain opaque.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// The film landing page.
    Home,
    /// Search with an initial query.
    Search {
        /// Initial search text.
        query: String,
    },
    /// A film detail page.
    Film {
        /// Opaque film identifier.
        id: String,
    },
    /// The player placeholder.
    Player {
        /// Opaque playback identifier.
        id: String,
    },
    /// Addon management.
    Addons,
}

impl Route {
    /// Parse a screenshot destination without interpreting identifiers.
    pub fn parse(value: &str) -> Result<Self, String> {
        let (name, argument) = value
            .split_once(':')
            .map_or((value, None), |(a, b)| (a, Some(b)));
        match (name, argument) {
            ("home", None) => Ok(Self::Home),
            ("addons", None) => Ok(Self::Addons),
            ("search", query) => Ok(Self::Search {
                query: query.unwrap_or_default().into(),
            }),
            ("film", Some(id)) if !id.is_empty() => Ok(Self::Film { id: id.into() }),
            ("player", Some(id)) if !id.is_empty() => Ok(Self::Player { id: id.into() }),
            _ => Err(format!("Invalid screenshot route: {value}")),
        }
    }
    /// The sidebar item this route highlights; Film and Player highlight nothing.
    pub fn destination(&self) -> Option<Destination> {
        match self {
            Self::Home => Some(Destination::Home),
            Self::Search { .. } => Some(Destination::Search),
            Self::Addons => Some(Destination::Addons),
            Self::Film { .. } | Self::Player { .. } => None,
        }
    }
    /// Every route but the player draws the sidebar.
    pub fn has_sidebar(&self) -> bool {
        !matches!(self, Self::Player { .. })
    }
}

/// A bounded browser history with stable identities for view caching.
#[derive(Clone, Debug)]
pub struct History {
    entries: Vec<(u64, Route)>,
    cursor: usize,
    next_id: u64,
}

impl History {
    /// Start with one destination.
    pub fn new(route: Route) -> Self {
        Self {
            entries: vec![(0, route)],
            cursor: 0,
            next_id: 1,
        }
    }
    /// Read the active route.
    pub fn current(&self) -> &Route {
        &self.entries[self.cursor].1
    }
    /// Read the active entry's stable identity.
    pub fn current_id(&self) -> u64 {
        self.entries[self.cursor].0
    }
    /// Whether backward navigation is possible.
    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }
    /// Whether forward navigation is possible.
    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }
    /// Push a distinct route, discarding forward entries and capping at 100.
    pub fn push(&mut self, route: Route) -> bool {
        if self.current() == &route {
            return false;
        }
        self.entries.truncate(self.cursor + 1);
        self.entries.push((self.next_id, route));
        self.next_id += 1;
        if self.entries.len() > 100 {
            self.entries.remove(0);
        }
        self.cursor = self.entries.len() - 1;
        true
    }
    /// Push `to` unless the current route already belongs to it, so a repeated sidebar
    /// selection never adds a history entry.
    pub fn go(&mut self, to: Destination) -> bool {
        self.current().destination() != Some(to) && self.push(to.route())
    }
    /// Replace the active route, giving the replacement a fresh identity.
    pub fn replace(&mut self, route: Route) -> bool {
        if self.current() == &route {
            return false;
        }
        self.entries[self.cursor] = (self.next_id, route);
        self.next_id += 1;
        true
    }
    /// Rewrite entry `id`'s route without changing its identity, so its view is kept.
    pub fn retitle(&mut self, id: u64, route: Route) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.0 == id) {
            entry.1 = route;
        }
    }
    /// Move backward if possible.
    pub fn back(&mut self) -> bool {
        if !self.can_back() {
            return false;
        }
        self.cursor -= 1;
        true
    }
    /// Move forward if possible.
    pub fn forward(&mut self) -> bool {
        if !self.can_forward() {
            return false;
        }
        self.cursor += 1;
        true
    }
    /// Test whether a cached entry still belongs to history.
    pub fn contains(&self, id: u64) -> bool {
        self.entries.iter().any(|entry| entry.0 == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_and_replace() {
        let mut history = History::new(Route::Home);
        assert!(!history.back());
        assert!(!history.forward());
        assert!(history.push(Route::Addons));
        let id = history.current_id();
        assert!(!history.push(Route::Addons));
        assert_eq!(history.current_id(), id);
        assert!(history.back());
        assert!(history.can_forward());
        assert!(history.forward());
        assert!(history.replace(Route::Film { id: "a:b".into() }));
        assert!(!history.contains(id));
        assert!(!history.replace(history.current().clone()));
        assert!(history.back());
        assert_eq!(history.current(), &Route::Home);
        assert!(history.forward());
        assert_eq!(history.current(), &Route::Film { id: "a:b".into() });
    }

    #[test]
    fn truncation_and_noop_preserve_forward() {
        let mut history = History::new(Route::Home);
        history.push(Route::Addons);
        history.back();
        assert!(!history.push(Route::Home));
        assert!(history.can_forward());
        history.push(Route::Search { query: "x".into() });
        assert!(!history.can_forward());
        assert_eq!(history.entries.len(), 2);
    }

    #[test]
    fn drops_oldest_at_cap() {
        let mut history = History::new(Route::Home);
        for i in 0..150 {
            history.push(Route::Film { id: i.to_string() });
        }
        assert_eq!(history.entries.len(), 100);
        for _ in 0..99 {
            assert!(history.back());
        }
        assert!(!history.back());
        assert_eq!(history.current(), &Route::Film { id: "50".into() });
    }

    #[test]
    fn destinations_round_trip() {
        for destination in Destination::ALL {
            assert_eq!(destination.route().destination(), Some(destination));
            assert!(destination.route().has_sidebar());
        }
        assert_eq!(
            Destination::Search.route(),
            Route::Search {
                query: String::new()
            }
        );
        let film = Route::Film { id: "a:b".into() };
        assert_eq!(film.destination(), None);
        assert!(film.has_sidebar());
        let player = Route::Player { id: "a:b".into() };
        assert_eq!(player.destination(), None);
        assert!(!player.has_sidebar());
    }

    #[test]
    fn go_never_duplicates_the_current_destination() {
        let mut history = History::new(Route::Search { query: "x".into() });
        assert!(!history.go(Destination::Search));
        assert_eq!(history.entries.len(), 1);
        assert!(history.go(Destination::Home));
        assert!(!history.go(Destination::Home));
        assert!(history.go(Destination::Addons));
        assert_eq!(history.entries.len(), 3);
        history.push(Route::Film { id: "a:b".into() });
        assert!(history.go(Destination::Home));
        assert_eq!(history.entries.len(), 5);
        assert!(history.back());
        assert_eq!(history.current(), &Route::Film { id: "a:b".into() });
    }

    #[test]
    fn retitle_keeps_identity() {
        let mut history = History::new(Route::Search {
            query: String::new(),
        });
        let id = history.current_id();
        history.retitle(id, Route::Search { query: "x".into() });
        assert_eq!(history.current_id(), id);
        assert_eq!(history.current(), &Route::Search { query: "x".into() });
    }

    #[test]
    fn screenshot_routes() {
        for value in [
            "home",
            "search",
            "search:",
            "search:a:b",
            "film:a:b",
            "player:a:b",
            "addons",
        ] {
            assert!(Route::parse(value).is_ok(), "{value}");
        }
        assert_eq!(
            Route::parse("film:a:b"),
            Ok(Route::Film { id: "a:b".into() })
        );
        for value in [
            "", "Home", "home:x", "addons:", "film", "film:", "player:", "unknown",
        ] {
            assert!(Route::parse(value).is_err(), "{value}");
        }
    }
}
