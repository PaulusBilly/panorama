use super::*;
use futures::{StreamExt, channel::mpsc};
use panorama_core::{
    addons::{ResourceEvent, StreamState, StreamsEvent},
    stremio::{CoreChange, FilmMeta},
};

impl Film {
    pub(super) fn load_details(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        self.loading = true;
        self.details_error = false;
        let id = self.id.clone();
        let addons = self.installed.clone();
        let hold = self.args.fixtures && self.args.film_loading;
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.clone().spawn(async move {
            if hold {
                return;
            }
            if let Some(client) = &services.addons {
                let mut events = client.details(&addons, &id, None);
                while let Some(event) = events.next().await {
                    if sender
                        .unbounded_send((event, client.metadata_base(&addons, &id).await))
                        .is_err()
                    {
                        break;
                    }
                }
            } else {
                let event = crate::fixtures::details(&id).map_or(
                    ResourceEvent::Failed(panorama_core::addons::FailureKind::InvalidInput),
                    ResourceEvent::Fresh,
                );
                let _ = sender.unbounded_send((event, None));
            }
        }));
        self.details_task = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some((event, meta_base)) = receiver.next().await {
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
    pub(super) fn load_sources(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        self.sources_loading = true;
        self.sources_error = false;
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
        let Some(session) = services.session.clone() else {
            if self.args.film_resume {
                self.resume = Some(panorama_core::stremio::ResumePoint {
                    offset_secs: 1200.0,
                    duration_secs: 6100.0,
                });
            }
            return;
        };
        let id = self.id.clone();
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.spawn(async move {
            let mut events = session.lock().await.subscribe();
            loop {
                let (saved, resume) = {
                    let session = session.lock().await;
                    (session.in_library(&id), session.resume(&id))
                };
                if sender.unbounded_send((saved, resume)).is_err() {
                    break;
                }
                loop {
                    match events.recv().await {
                        Ok(CoreChange::LibraryChanged | CoreChange::ProfileChanged)
                        | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        _ => {}
                    }
                }
            }
        }));
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
