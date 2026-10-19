use crate::{router::Route, transition::Tween};
use std::time::{Duration, Instant};

/// Search disclosure and query handoff, independent of the editing control.
pub struct SearchBand {
    pub(crate) open: bool,
    pub(crate) query: String,
    pub(crate) motion: Tween,
    pub(crate) hold_until: Instant,
}

impl SearchBand {
    /// Initialize the disclosure for the starting route.
    pub fn new(route: &Route, open_home: bool) -> Self {
        let open = open_home || matches!(route, Route::Search { .. });
        Self {
            open,
            query: match route {
                Route::Search { query } => query.clone(),
                _ => String::new(),
            },
            motion: Tween::fixed(if open { 1.0 } else { 0.0 }),
            hold_until: Instant::now() + Duration::from_millis(400),
        }
    }
    /// Open with the route query, or close without changing it.
    pub fn toggle(&mut self, route: &Route) {
        if !self.open
            && let Route::Search { query } = route
        {
            self.query.clone_from(query);
        }
        self.set_open(!self.open);
    }
    /// Clear the disclosure and return Home.
    pub fn clear(&mut self) -> Route {
        self.query.clear();
        self.set_open(false);
        Route::Home
    }
    /// Trim editing text and choose the submitted destination.
    pub fn submit(&mut self, value: &str) -> Route {
        self.query = value.trim().into();
        if self.query.is_empty() {
            self.clear()
        } else {
            self.set_open(true);
            Route::Search {
                query: self.query.clone(),
            }
        }
    }
    pub(crate) fn set_open(&mut self, open: bool) {
        self.open = open;
        if open {
            self.hold_until = Instant::now() + Duration::from_millis(400);
        }
        self.motion.retarget(
            if open { 1.0 } else { 0.0 },
            Duration::from_millis(320),
            Some([0.22, 1.0, 0.36, 1.0]),
            Instant::now(),
        );
    }
    pub(crate) fn height(&self, width: f32, reduced: bool) -> f32 {
        let size = if width <= 700.0 {
            (width * 0.08 + 12.0).clamp(40.0, 48.0)
        } else {
            (width * 0.05 + 12.0).clamp(48.0, 64.0)
        };
        let full = size * 1.2 + if width <= 700.0 { 73.0 } else { 93.0 };
        full * self.motion.value(Instant::now(), reduced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disclosure_submit_clear_and_escape() {
        let mut band = SearchBand::new(&Route::Home, false);
        assert!(!band.open);
        band.toggle(&Route::Home);
        assert!(band.open);
        assert_eq!(
            band.submit("  past  "),
            Route::Search {
                query: "past".into()
            }
        );
        band.toggle(&Route::Search {
            query: "past".into(),
        });
        assert!(!band.open);
        assert_eq!(band.query, "past");
        band.toggle(&Route::Search {
            query: "night".into(),
        });
        assert_eq!(band.query, "night");
        assert_eq!(band.clear(), Route::Home);
        assert!(!band.open && band.query.is_empty());
        band.toggle(&Route::Home);
        assert_eq!(band.submit(" \n "), Route::Home);
        assert!(!band.open);
    }
    #[test]
    fn route_initial_query_and_replacement_handoff() {
        let mut band = SearchBand::new(
            &Route::Search {
                query: "past".into(),
            },
            false,
        );
        assert!(band.open);
        assert_eq!(band.query, "past");
        let mut history = crate::router::History::new(Route::Home);
        history.push(band.submit("past"));
        history.replace(band.submit("night"));
        assert!(history.back());
        assert_eq!(history.current(), &Route::Home);
    }
}
