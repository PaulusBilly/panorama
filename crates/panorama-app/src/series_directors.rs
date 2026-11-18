//! Episode directors for series cards. Addons send no director for a series, so cards on screen
//! ask TMDB (with the user's key) one show at a time and redraw as each answer lands.

use crate::services::Services;
use gpui::App;
use panorama_core::addons::{FilmDetails, Kind};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, LazyLock, Mutex},
};

#[derive(Default)]
struct Lookup {
    known: HashMap<String, Arc<[String]>>,
    asked: HashSet<String>,
    queue: VecDeque<(Kind, String)>,
    running: bool,
}

// ponytail: one process-wide table, unbounded; a session sees a few hundred shows at most.
static LOOKUP: LazyLock<Mutex<Lookup>> = LazyLock::new(Mutex::default);

fn lookup() -> std::sync::MutexGuard<'static, Lookup> {
    LOOKUP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The directors a card shows: the addon's, or the episode directors TMDB has sent for a series.
pub fn names(film: &FilmDetails) -> Option<Arc<[String]>> {
    if !film.director.is_empty() {
        return Some(film.director.clone().into());
    }
    (film.kind != Kind::Movie)
        .then(|| lookup().known.get(&film.id).cloned())
        .flatten()
}

/// Ask once for a series card's directors; nothing happens for films or credited series.
pub fn want(film: &FilmDetails, services: Option<&Arc<Services>>, cx: &mut App) {
    let Some(services) = services else { return };
    if film.kind == Kind::Movie || !film.director.is_empty() {
        return;
    }
    let mut table = lookup();
    if !table.asked.insert(film.id.clone()) {
        return;
    }
    table.queue.push_back((film.kind, film.id.clone()));
    if std::mem::replace(&mut table.running, true) {
        return;
    }
    drop(table);
    let services = services.clone();
    cx.spawn(async move |cx| {
        loop {
            let next = {
                let mut table = lookup();
                let next = table.queue.pop_front();
                table.running = next.is_some();
                next
            };
            let Some((kind, id)) = next else { break };
            // `knows` skips the Wikidata fallback, which has no directors.
            let mut job = services.titles(kind, id.clone(), true);
            let directors = (&mut job.0)
                .await
                .ok()
                .flatten()
                .map(|titles| titles.directors);
            if let Some(directors) = directors.filter(|names| !names.is_empty()) {
                lookup().known.insert(id, directors.into());
                cx.update(|cx| cx.refresh_windows());
            }
        }
    })
    .detach();
}
