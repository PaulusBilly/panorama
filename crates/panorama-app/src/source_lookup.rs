//! One lookup per film, shared by the hover prefetch and the film page, so a click joins the
//! request already running instead of asking the addons again.
use crate::{
    app_state::AppState,
    services::{Job, Services},
};
use futures::{StreamExt, channel::mpsc};
use gpui::{Context, Task};
use panorama_core::addons::{Kind, StreamGroup, StreamsEvent};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

/// Never written to disk: links expire. This long is safe to reuse, Retry asks again.
const FRESH: Duration = Duration::from_secs(10 * 60);

/// What the addons have answered for one film so far.
pub struct SourceLookup {
    pub groups: Arc<Vec<StreamGroup>>,
    pub finished: bool,
    /// The lookup itself failed, as when offline.
    pub failed: bool,
    started: Instant,
    _task: Task<()>,
}

#[derive(Default)]
pub struct SourceLookups {
    films: HashMap<String, SourceLookup>,
}

impl SourceLookups {
    pub fn get(&self, id: &str) -> Option<&SourceLookup> {
        self.films.get(id)
    }
    fn fresh(&self, id: &str, now: Instant) -> bool {
        self.get(id)
            .is_some_and(|lookup| now.duration_since(lookup.started) < FRESH)
    }
    /// Background lookups run one at a time: AIOStreams takes seconds and a pause costs a request.
    fn busy(&self) -> bool {
        self.films.values().any(|lookup| !lookup.finished)
    }
}

/// How a fixture run answers, so captures can hold the page or show no sources.
#[derive(Clone, Copy, Default)]
pub struct FixtureSources {
    pub hold: bool,
    pub none: bool,
}

impl AppState {
    /// The film page's lookup: reuses a fresh or running one unless `again` forces a new ask.
    pub fn open_sources(
        &mut self,
        kind: Kind,
        id: &str,
        again: bool,
        fixture: FixtureSources,
        cx: &mut Context<Self>,
    ) {
        if again || !self.source_lookups.fresh(id, Instant::now()) {
            self.start_sources(kind, id, fixture, cx);
        }
        self.streams_for = Some(id.to_owned());
        if let Some(lookup) = self.source_lookups.get(id).filter(|l| !l.groups.is_empty()) {
            self.streams = Some((id.to_owned(), lookup.groups.clone()));
        }
    }

    /// A film's, ahead of a click, at most one at a time.
    pub fn prefetch_sources(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.source_lookups.busy() && !self.source_lookups.fresh(id, Instant::now()) {
            self.start_sources(Kind::Movie, id, FixtureSources::default(), cx);
        }
    }

    fn start_sources(
        &mut self,
        kind: Kind,
        id: &str,
        fixture: FixtureSources,
        cx: &mut Context<Self>,
    ) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let now = Instant::now();
        self.source_lookups
            .films
            .retain(|_, lookup| now.duration_since(lookup.started) < FRESH);
        let task = lookup_task(
            services,
            self.installed.clone(),
            kind,
            id.to_owned(),
            fixture,
            cx,
        );
        self.source_lookups.films.insert(
            id.to_owned(),
            SourceLookup {
                groups: Arc::default(),
                finished: false,
                failed: false,
                started: now,
                _task: task,
            },
        );
        cx.notify();
    }
}

fn lookup_task(
    services: Arc<Services>,
    addons: Vec<panorama_core::stremio::Descriptor>,
    kind: Kind,
    id: String,
    fixture: FixtureSources,
    cx: &mut Context<AppState>,
) -> Task<()> {
    let (sender, mut receiver) = mpsc::unbounded();
    let job = Job(services.runtime.clone().spawn({
        let id = id.clone();
        async move {
            if fixture.hold {
                std::future::pending::<()>().await;
            } else if let Some(client) = &services.addons {
                let mut events = client.streams(&addons, kind, &id);
                while let Some(event) = events.next().await {
                    if sender.unbounded_send(event).is_err() {
                        break;
                    }
                }
            } else {
                let _ = sender.unbounded_send(crate::fixtures::streams(fixture.none));
            }
        }
    }));
    cx.spawn(async move |state, cx| {
        let _job = job;
        while let Some(event) = receiver.next().await {
            let updated = state.update(cx, |state, cx| {
                let watched = state.streams_for.as_deref() == Some(id.as_str());
                let Some(lookup) = state.source_lookups.films.get_mut(&id) else {
                    return;
                };
                match event {
                    StreamsEvent::Groups(groups) => {
                        lookup.groups = Arc::new(groups);
                        if watched {
                            state.streams = Some((id.clone(), lookup.groups.clone()));
                        }
                    }
                    StreamsEvent::Failed(_) => lookup.failed = true,
                    StreamsEvent::NoSources => {}
                }
                cx.notify();
            });
            if updated.is_err() {
                return;
            }
        }
        let _ = state.update(cx, |state, cx| {
            if let Some(lookup) = state.source_lookups.films.get_mut(&id) {
                lookup.finished = true;
            }
            cx.notify();
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(age: Duration, finished: bool, now: Instant) -> SourceLookup {
        SourceLookup {
            groups: Arc::default(),
            finished,
            failed: false,
            started: now - age,
            _task: Task::ready(()),
        }
    }

    #[test]
    fn a_lookup_is_reused_for_ten_minutes_and_one_running_blocks_background_ones() {
        let now = Instant::now();
        let mut lookups = SourceLookups::default();
        assert!(!lookups.fresh("a", now));
        lookups
            .films
            .insert("a".into(), lookup(Duration::from_secs(599), true, now));
        assert!(lookups.fresh("a", now));
        assert!(!lookups.busy());
        lookups
            .films
            .insert("b".into(), lookup(Duration::from_secs(600), false, now));
        assert!(!lookups.fresh("b", now));
        assert!(lookups.busy());
    }
}
