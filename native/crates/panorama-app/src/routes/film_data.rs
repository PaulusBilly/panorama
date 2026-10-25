use super::*;
use futures::{StreamExt, channel::mpsc};
use panorama_core::{
    addons::{ResourceEvent, StreamState, StreamsEvent},
    stremio::FilmMeta,
};

impl Film {
    pub(super) fn load_details(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        self.loading = true;
        self.details_error = false;
        let id = self.id.clone();
        let hold = self.args.fixtures && self.args.film_loading;
        let (job, mut receiver) = if hold {
            let (_held, receiver) = mpsc::unbounded();
            (None, receiver)
        } else {
            let (job, receiver) = services.details(id.clone());
            (Some(job), receiver)
        };
        self.details_task = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some(event) = receiver.next().await {
                let meta_base = {
                    let (services, id) = (services.clone(), id.clone());
                    let lookup = services
                        .runtime
                        .clone()
                        .spawn(async move { services.metadata_base(&id).await });
                    lookup.await.ok().flatten()
                };
                if entity
                    .update(cx, |film, cx| {
                        film.meta_base = meta_base;
                        film.loading = false;
                        match event {
                            ResourceEvent::CacheHit(meta) | ResourceEvent::Fresh(meta) => {
                                film.metadata = Some(meta);
                                film.details_error = false;
                            }
                            ResourceEvent::Failed(_) => {
                                film.details_error = film.metadata.is_none()
                            }
                        }
                        // Awards wait for real details: the card's preview lacks the director
                        // that tells MUBI's namesakes apart.
                        if !film.details_error && film.metadata.is_some() {
                            film.load_credits(cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        cx.notify();
    }
    /// Awards; the cast and crew come with the metadata. Without either the section is left
    /// out. Waits for the details, whose title, year and director find the film on MUBI; the
    /// answer is kept in the store so a film costs MUBI two requests a month at most.
    pub(super) fn load_credits(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        let Some(wikidata) = services.credits.clone() else {
            if self.args.fixtures {
                self.awards = crate::fixtures::awards();
            }
            return;
        };
        if self.loading || self.credits_task.is_some() {
            return;
        }
        let id = self.id.clone();
        let meta = self.metadata.clone();
        let (mubi, store) = (services.mubi.clone(), services.store.clone());
        let mut job = Job(services.runtime.spawn(async move {
            let key = panorama_core::store::Key::awards(&id).ok();
            let cached = store
                .as_ref()
                .zip(key.as_ref())
                .and_then(|(store, key)| store.get(key).ok().flatten())
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
            if let Some(awards) = cached {
                return awards;
            }
            let (awards, settled) =
                fetch_awards(&id, meta.as_ref(), mubi.as_ref(), &wikidata).await;
            // A network failure is retried on the next visit; any answer, even none, is kept.
            if settled
                && let (Some(store), Some(key)) = (&store, &key)
                && let Ok(bytes) = serde_json::to_vec(&awards)
            {
                let _ = store.set(key, &bytes);
            }
            awards
        }));
        self.credits_task = Some(cx.spawn(async move |entity, cx| {
            if let Ok(awards) = (&mut job.0).await {
                let _ = entity.update(cx, |film, cx| {
                    film.awards = awards;
                    cx.notify();
                });
            }
        }));
    }
    pub(super) fn load_sources(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        self.sources_loading = true;
        self.sources_error = false;
        self.look_up_subtitles(&services, cx);
        self.groups.clear();
        self.selected = None;
        let addons = self.installed.clone();
        let id = self.id.clone();
        let no_sources = self.args.no_sources;
        let hold = self.args.fixtures && self.args.film_loading;
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.clone().spawn(async move {
            if let Some(client) = &services.addons {
                let mut events = client.streams(&addons, &id);
                while let Some(event) = events.next().await {
                    if sender.unbounded_send(event).is_err() {
                        break;
                    }
                }
            } else {
                if !hold {
                    let _ = sender.unbounded_send(crate::fixtures::streams(no_sources));
                }
            }
        }));
        self.sources_task = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some(event) = receiver.next().await {
                if entity
                    .update(cx, |film, cx| {
                        film.sources_loading = false;
                        match event {
                            StreamsEvent::Groups(groups) => {
                                film.sources_error = groups
                                    .iter()
                                    .any(|group| matches!(group.state, StreamState::Failed(_)));
                                film.selected = crate::playback::selection::first_source(&groups);
                                film.groups = groups;
                                let shared = (film.id.clone(), Arc::new(film.groups.clone()));
                                film.state.update(cx, |state, cx| {
                                    state.streams = Some(shared);
                                    cx.notify();
                                });
                            }
                            StreamsEvent::Failed(_) => film.sources_error = true,
                            StreamsEvent::NoSources => {}
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        cx.notify();
    }
    /// Ask the addons for subtitle files alongside the sources. Fixture runs have none.
    fn look_up_subtitles(
        &mut self,
        services: &Arc<crate::services::Services>,
        cx: &mut Context<Self>,
    ) {
        self.subtitles = None;
        let shared = crate::playback::launch::subtitle_lookup(
            services,
            self.installed.clone(),
            self.id.clone(),
        );
        self.subtitle_lookup = Some(shared.clone());
        self.subtitles_task = Some(cx.spawn(async move |entity, cx| {
            let files = shared.await;
            let _ = entity.update(cx, |film, cx| {
                film.subtitles = Some(files);
                cx.notify();
            });
        }));
    }
    pub(super) fn subscribe_library(&mut self, cx: &mut Context<Self>) {
        self.library_task = None;
        self.saved = false;
        self.resume = None;
        if !matches!(self.account, Account::SignedIn(_)) {
            return;
        }
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        if services.session.is_none() {
            if self.args.film_resume {
                self.resume = Some(panorama_core::stremio::ResumePoint {
                    offset_secs: 1200.0,
                    duration_secs: 6100.0,
                });
            }
            return;
        }
        let (job, mut receiver) = services.library_flags(self.id.clone());
        self.library_task = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some((saved, resume)) = receiver.next().await {
                if entity
                    .update(cx, |film, cx| {
                        film.resume = resume;
                        film.set_saved(saved);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    pub(super) fn set_saved(&mut self, saved: bool) {
        if saved != self.saved {
            self.saved = saved;
        }
    }
    pub(super) fn toggle_watchlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.account, Account::SignedIn(_)) {
            self.sign_in(window, cx);
            return;
        }
        if self.saving {
            return;
        }
        let Some(meta) = self.metadata.clone() else {
            return;
        };
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        let saved = !self.saved;
        self.saving = true;
        self.watchlist_error = false;
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.clone().spawn(async move {
            let result = if let Some(session) = &services.session {
                let film = FilmMeta {
                    id: meta.id.clone(),
                    name: meta.name.clone(),
                    poster: meta.poster.clone(),
                };
                session.lock().await.set_in_library(&film, saved).is_ok()
            } else {
                true
            };
            let _ = sender.unbounded_send(result);
        }));
        self.save_task = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            if let Some(success) = receiver.next().await {
                let _ = entity.update(cx, |film, cx| {
                    film.saving = false;
                    film.watchlist_error = !success;
                    if success {
                        film.set_saved(saved);
                    }
                    cx.notify();
                });
            }
        }));
        cx.notify();
    }
    pub(super) fn sign_in(&self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.open_login(window, cx));
    }
}

/// MUBI's awards and festivals for the film, else Wikidata's awards, and whether every source
/// asked gave an answer rather than a network failure.
async fn fetch_awards(
    id: &str,
    meta: Option<&FilmDetails>,
    mubi: Option<&panorama_core::mubi::MubiClient>,
    wikidata: &panorama_core::credits::CreditsClient,
) -> (Vec<panorama_core::credits::Award>, bool) {
    use panorama_core::credits::CreditsError;
    let mut settled = true;
    if let (Some(mubi), Some(meta)) = (mubi, meta) {
        let query = panorama_core::mubi::FilmQuery {
            title: &meta.name,
            original_title: meta.original_name.as_deref(),
            year: meta
                .release_info
                .as_deref()
                .and_then(super::super::festival::release_year),
            directors: &meta.director,
        };
        match mubi.find_film(&query).await {
            Ok(Some(film)) => match mubi.awards(film).await {
                Ok(awards) if !awards.is_empty() => return (awards, true),
                Ok(_) => {}
                Err(_) => settled = false,
            },
            Ok(None) => {}
            Err(_) => settled = false,
        }
    }
    match wikidata.awards(id).await {
        Ok(awards) => (awards, settled),
        // An id Wikidata cannot be asked about has no awards there, for good; for one it can,
        // an unreadable answer is retried on the next visit.
        Err(CreditsError::Invalid) => {
            let askable = id.starts_with("tt") || id.starts_with("tmdb:");
            (vec![], settled && !askable)
        }
        Err(CreditsError::Network) => (vec![], false),
    }
}
