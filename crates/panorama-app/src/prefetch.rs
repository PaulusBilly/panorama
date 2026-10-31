//! Warming a film's details and artwork once the pointer rests on its card.
use crate::app_state::AppState;
use futures::StreamExt;
use gpui::{App, Entity, Task};
use panorama_core::addons::{Kind, ResourceEvent};
use std::{collections::HashSet, time::Duration};

/// Long enough that sweeping across a row asks the addons for nothing.
const INTENT: Duration = Duration::from_millis(120);
/// Sources cost a slow request to every stream addon, so they wait for a real pause.
const SOURCE_INTENT: Duration = Duration::from_millis(300);

/// Which films are warm this session, and the one being warmed. Moving to another card drops
/// the task, cancelling its requests, so at most one film is ever being fetched.
#[derive(Default)]
pub struct Prefetch {
    warmed: HashSet<String>,
    current: Option<(String, Task<()>)>,
    pause: Option<Task<()>>,
}

impl Prefetch {
    fn wants(&self, id: &str) -> bool {
        !self.warmed.contains(id)
            && self
                .current
                .as_ref()
                .is_none_or(|(current, _)| current != id)
    }
}

/// Details and the foreign title land in the store and artwork on disk, where the film page
/// finds them. A longer pause also starts the source lookup, which the page then joins.
pub fn hovered(state: &Entity<AppState>, kind: Kind, id: &str, cx: &mut App) {
    state.update(cx, |state, cx| {
        let film = id.to_owned();
        // A show's sources belong to an episode, which only its page settles.
        if kind == Kind::Movie {
            state.prefetch.pause = Some(cx.spawn(async move |state, cx| {
                cx.background_executor().timer(SOURCE_INTENT).await;
                let _ = state.update(cx, |state, cx| state.prefetch_sources(&film, cx));
            }));
        }
        if !state.prefetch.wants(id) {
            return;
        }
        let Some(services) = state.services.clone() else {
            return;
        };
        let film = id.to_owned();
        let task = cx.spawn(async move |state, cx| {
            cx.background_executor().timer(INTENT).await;
            let (_job, mut events) = services.details(kind, film.clone());
            let mut details = None;
            while let Some(event) = events.next().await {
                if let ResourceEvent::CacheHit(meta) | ResourceEvent::Fresh(meta) = event {
                    details = Some(meta);
                }
            }
            let mut title = services.original_title(film.clone());
            if let Some(details) = details {
                let mut artwork = services.warm_artwork(&details);
                let _ = (&mut artwork.0).await;
            }
            let _ = (&mut title.0).await;
            let _ = state.update(cx, |state, _| {
                state.prefetch.warmed.insert(film);
            });
        });
        state.prefetch.current = Some((id.to_owned(), task));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_film_is_warmed_once_and_only_one_at_a_time_runs() {
        let mut prefetch = Prefetch::default();
        assert!(prefetch.wants("a"));
        prefetch.current = Some(("a".into(), Task::ready(())));
        assert!(!prefetch.wants("a"));
        assert!(prefetch.wants("b"));
        prefetch.warmed.insert("a".into());
        prefetch.current = None;
        assert!(!prefetch.wants("a"));
    }
}
