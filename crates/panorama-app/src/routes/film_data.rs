use super::*;
use futures::{StreamExt, channel::mpsc};
use panorama_core::{
    addons::{ResourceEvent, StreamState},
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
            let (job, receiver) = services.details(self.kind, id.clone());
            (Some(job), receiver)
        };
        let kind = self.kind;
        self.details_task = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some(event) = receiver.next().await {
                let meta_base = {
                    let (services, id) = (services.clone(), id.clone());
                    let lookup = services
                        .runtime
                        .clone()
                        .spawn(async move { services.metadata_base(kind, &id).await });
                    lookup.await.ok().flatten()
                };
                if entity
                    .update(cx, |film, cx| {
                        film.meta_base = meta_base;
                        film.loading = false;
                        match event {
                            ResourceEvent::CacheHit(meta) | ResourceEvent::Fresh(meta) => {
                                let mut meta = trusted(film.card.as_ref(), meta);
                                meta.original_name =
                                    meta.original_name.or_else(|| film.original_title.clone());
                                film.set_episodes(std::mem::take(&mut meta.episodes));
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
                            film.load_original_title(cx);
                        }
                        film.load_watched(cx);
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
        if self.kind != Kind::Movie {
            return;
        }
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
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            let cached = store
                .as_ref()
                .zip(key.as_ref())
                .and_then(|(store, key)| store.get(key).ok().flatten())
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|value| fresh_awards(&value, now));
            if let Some(awards) = cached {
                return awards;
            }
            let (awards, settled) =
                fetch_awards(&id, meta.as_ref(), mubi.as_ref(), &wikidata).await;
            // A network failure is retried on the next visit; any answer, even none, is kept.
            if settled
                && let (Some(store), Some(key)) = (&store, &key)
                && let Ok(bytes) =
                    serde_json::to_vec(&serde_json::json!({ "fetched": now, "awards": awards }))
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
    fn load_original_title(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        let known = self
            .metadata
            .as_ref()
            .is_none_or(|meta| meta.original_name.is_some());
        if known || self.original_task.is_some() {
            return;
        }
        self.original_pending = true;
        let mut job = services.original_title(self.id.clone());
        self.original_task = Some(cx.spawn(async move |entity, cx| {
            let title = (&mut job.0).await.ok().flatten();
            let _ = entity.update(cx, |film, cx| {
                film.original_pending = false;
                if let Some(title) = title {
                    if let Some(meta) = &mut film.metadata {
                        meta.original_name.get_or_insert_with(|| title.clone());
                    }
                    film.original_title = Some(title);
                }
                cx.notify();
            });
        }));
    }
    /// `again` asks the addons afresh; otherwise a lookup hovering already began or finished
    /// is used as it stands.
    pub(super) fn load_sources(&mut self, again: bool, cx: &mut Context<Self>) {
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        let Some(id) = self.target.clone() else {
            return;
        };
        self.look_up_subtitles(&services, id.clone(), cx);
        let fixture = crate::source_lookup::FixtureSources {
            hold: self.args.fixtures && self.args.film_loading,
            none: self.args.no_sources,
        };
        self.sources_seen = None;
        self.sources_loading = true;
        self.sources_error = false;
        self.groups.clear();
        self.selected = None;
        let kind = self.kind;
        self.state.update(cx, |state, cx| {
            state.open_sources(kind, &id, again, fixture, cx);
            cx.notify();
        });
        cx.notify();
    }
    /// Take what the shared lookup has so far. One playable source is enough to show the page.
    pub(super) fn sync_sources(&mut self, state: &crate::app_state::AppState) {
        let Some(lookup) = (self.target.as_ref()).and_then(|id| state.source_lookups.get(id))
        else {
            return;
        };
        let seen = self
            .sources_seen
            .as_ref()
            .is_some_and(|seen| Arc::ptr_eq(seen, &lookup.groups));
        if !seen {
            self.sources_seen = Some(lookup.groups.clone());
            self.groups = (*lookup.groups).clone();
            self.selected = crate::playback::selection::first_source(&self.groups);
        }
        self.sources_error = lookup.failed
            || self
                .groups
                .iter()
                .any(|group| matches!(group.state, StreamState::Failed(_)));
        if lookup.finished || self.selected.is_some() {
            self.sources_loading = false;
        }
    }
    /// Ask the addons for subtitle files alongside the sources. Fixture runs have none.
    fn look_up_subtitles(
        &mut self,
        services: &Arc<crate::services::Services>,
        id: String,
        cx: &mut Context<Self>,
    ) {
        self.subtitles = None;
        let shared = crate::playback::launch::subtitle_lookup(
            services,
            self.installed.clone(),
            self.kind,
            id,
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
                    video_id: crate::fixtures::resume_episode(&self.id),
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
                        film.load_watched(cx);
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
                    kind: meta.kind,
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

/// Details outrank the card, unless they are blank where the card is not. Metadata addons can
/// answer an id with a different film of the same name, as a bare short with no year or director.
fn trusted(card: Option<&FilmDetails>, details: FilmDetails) -> FilmDetails {
    let blank = |film: &FilmDetails| {
        film.director.is_empty() && film.release_info.as_deref().is_none_or(str::is_empty)
    };
    match card {
        Some(card) if blank(&details) && !blank(card) => FilmDetails {
            episodes: details.episodes,
            ..card.clone()
        },
        _ => details,
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

/// A film keeps gathering festival entries and prizes, so cached awards are looked up again
/// after two weeks.
const AWARDS_FRESH_SECS: u64 = 14 * 24 * 60 * 60;

/// Cached awards still fresh at `now`; anything else, including the older undated form, is
/// looked up again.
fn fresh_awards(
    cached: &serde_json::Value,
    now: u64,
) -> Option<Vec<panorama_core::credits::Award>> {
    let fetched = cached["fetched"].as_u64()?;
    if now.saturating_sub(fetched) > AWARDS_FRESH_SECS {
        return None;
    }
    serde_json::from_value(cached["awards"].clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_details_never_replace_a_richer_card() {
        let mut card = crate::fixtures::films().remove(0);
        card.description = Some("The film on the card".into());
        let mut blank = card.clone();
        blank.description = Some("Another film".into());
        blank.director = vec![];
        blank.release_info = Some(String::new());
        assert_eq!(trusted(Some(&card), blank.clone()), card);
        assert_eq!(trusted(None, blank.clone()), blank);
        let mut richer = card.clone();
        richer.description = Some("Fuller synopsis".into());
        assert_eq!(trusted(Some(&card), richer.clone()), richer);
    }
    #[test]
    fn cached_awards_expire_after_two_weeks() {
        let cached = serde_json::json!({ "fetched": 1_000, "awards": [] });
        assert_eq!(fresh_awards(&cached, 1_000), Some(vec![]));
        assert_eq!(
            fresh_awards(&cached, 1_000 + AWARDS_FRESH_SECS),
            Some(vec![])
        );
        assert_eq!(fresh_awards(&cached, 1_001 + AWARDS_FRESH_SECS), None);
        // The undated form from before is looked up again.
        assert_eq!(fresh_awards(&serde_json::json!([]), 1_000), None);
    }
}
