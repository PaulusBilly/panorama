use crate::services::Services;
use futures::StreamExt;
use gpui::{Context, Task};
use panorama_core::{
    addons::{FilmDetails, ResourceEvent},
    stremio::{ContinueItem, FilmMeta, ResumePoint},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// What the Continue row and the hero render. `film` starts as a stub built from the library item
/// (name, poster) and is replaced by addon details as they arrive.
#[derive(Clone, Debug)]
pub struct ContinueCard {
    pub film: FilmDetails,
    pub resume: ResumePoint,
    pub progress: f32,
}

/// Owned by AppState and shared by every Home history entry. The single writer of continue state.
pub struct ContinueWatching {
    items: Arc<[ContinueItem]>,
    details: HashMap<String, FilmDetails>,
    /// One in-flight details request per id; dropping a task aborts its job.
    requested: HashMap<String, Task<()>>,
    /// Derived from `items` and `details` by `rebuild`, never written elsewhere.
    cards: Arc<[ContinueCard]>,
    library: Option<Task<()>>,
    services: Option<Arc<Services>>,
}

impl ContinueWatching {
    /// Fixtures (no session) show `fixtures::continue_items`; without services the row stays empty.
    pub fn new(services: Option<Arc<Services>>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            items: Arc::new([]),
            details: HashMap::new(),
            requested: HashMap::new(),
            cards: Arc::new([]),
            library: None,
            services: None,
        };
        this.restart(services, cx);
        this
    }

    pub fn restart(&mut self, services: Option<Arc<Services>>, cx: &mut Context<Self>) {
        self.library = None;
        self.services = None;
        self.requested.clear();
        self.set_items(vec![], cx);
        let Some(services) = services else {
            return;
        };
        let (job, mut receiver) = services.continue_watching();
        self.library = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some(items) = receiver.next().await {
                if entity
                    .update(cx, |this, cx| this.set_items(items, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        self.services = Some(services);
    }

    pub fn cards(&self) -> Arc<[ContinueCard]> {
        self.cards.clone()
    }

    fn set_items(&mut self, items: Vec<ContinueItem>, cx: &mut Context<Self>) {
        let Some(wanted) = reconcile(&self.items, &items, &self.details, &self.requested) else {
            return;
        };
        let keep: HashSet<&str> = items.iter().map(|item| item.film.id.as_str()).collect();
        self.requested.retain(|id, _| keep.contains(id.as_str()));
        self.items = items.into();
        for id in wanted {
            self.request(id, cx);
        }
        self.rebuild();
        cx.notify();
    }

    fn request(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let (job, mut receiver) = services.details(id.clone());
        let key = id.clone();
        let task = cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some(event) = receiver.next().await {
                let (ResourceEvent::CacheHit(details) | ResourceEvent::Fresh(details)) = event
                else {
                    // The stub stays; no retry until the id leaves the list and comes back.
                    continue;
                };
                let id = id.clone();
                if entity
                    .update(cx, |this, cx| {
                        this.details.insert(id, details);
                        this.rebuild();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        self.requested.insert(key, task);
    }

    fn rebuild(&mut self) {
        self.cards = build_cards(&self.items, &self.details);
    }
}

/// Ids that need a details request, or `None` when `next` changes nothing.
fn reconcile<T>(
    current: &[ContinueItem],
    next: &[ContinueItem],
    details: &HashMap<String, FilmDetails>,
    requested: &HashMap<String, T>,
) -> Option<Vec<String>> {
    if current == next {
        return None;
    }
    Some(
        next.iter()
            .map(|item| &item.film.id)
            .filter(|id| !details.contains_key(*id) && !requested.contains_key(*id))
            .cloned()
            .collect(),
    )
}

fn build_cards(
    items: &[ContinueItem],
    details: &HashMap<String, FilmDetails>,
) -> Arc<[ContinueCard]> {
    items
        .iter()
        .map(|item| ContinueCard {
            film: details
                .get(&item.film.id)
                .cloned()
                .unwrap_or_else(|| stub(&item.film)),
            resume: item.resume,
            progress: item.progress(),
        })
        .collect()
}

fn stub(meta: &FilmMeta) -> FilmDetails {
    FilmDetails {
        id: meta.id.clone(),
        name: meta.name.clone(),
        release_info: None,
        runtime: None,
        genres: vec![],
        description: None,
        director: vec![],
        origin_country: None,
        original_name: None,
        cast: vec![],
        poster: meta.poster.clone(),
        background: None,
        logo: None,
        imdb_rating: None,
        links: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, offset_secs: f64) -> ContinueItem {
        ContinueItem {
            film: FilmMeta {
                id: id.into(),
                name: id.to_uppercase(),
                poster: None,
            },
            resume: ResumePoint {
                offset_secs,
                duration_secs: 100.0,
            },
        }
    }

    #[test]
    fn identical_items_change_nothing() {
        let items = vec![item("a", 10.0), item("b", 20.0)];
        let none = HashMap::<String, ()>::new();
        assert_eq!(reconcile(&items, &items, &HashMap::new(), &none), None);
    }

    #[test]
    fn requests_only_ids_without_details_or_a_job() {
        let current = vec![item("a", 10.0)];
        let next = vec![
            item("a", 10.0),
            item("b", 20.0),
            item("c", 30.0),
            item("d", 40.0),
        ];
        let details = HashMap::from([("b".to_string(), stub(&next[1].film))]);
        let requested = HashMap::from([("c".to_string(), ())]);
        assert_eq!(
            reconcile(&current, &next, &details, &requested),
            Some(vec!["a".into(), "d".into()])
        );
    }

    #[test]
    fn progress_changes_are_applied() {
        let current = vec![item("a", 10.0)];
        let next = vec![item("a", 50.0)];
        let details = HashMap::from([("a".to_string(), stub(&next[0].film))]);
        assert_eq!(
            reconcile(&current, &next, &details, &HashMap::<String, ()>::new()),
            Some(vec![])
        );
    }

    #[test]
    fn cards_prefer_details_and_fall_back_to_the_stub() {
        let items = vec![item("a", 25.0), item("b", 50.0)];
        let mut enriched = stub(&items[0].film);
        enriched.description = Some("plot".into());
        let cards = build_cards(&items, &HashMap::from([("a".to_string(), enriched)]));
        assert_eq!(cards[0].film.description.as_deref(), Some("plot"));
        assert_eq!(cards[1].film.description, None);
        assert_eq!(cards[1].film.name, "B");
        assert_eq!((cards[0].progress, cards[1].progress), (0.25, 0.5));
    }
}
