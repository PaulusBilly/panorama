use super::*;
use futures::{StreamExt, channel::mpsc};
use panorama_core::{
    addons::{ResourceEvent, StreamState, StreamsEvent},
    stremio::CoreChange,
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
                    if sender.unbounded_send(event).is_err() {
                        break;
                    }
                }
            } else {
                let event = crate::fixtures::details(&id).map_or(
                    ResourceEvent::Failed(panorama_core::addons::FailureKind::InvalidInput),
                    ResourceEvent::Fresh,
                );
                let _ = sender.unbounded_send(event);
            }
        }));
        self.details_task = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some(event) = receiver.next().await {
                if entity
                    .update(cx, |film, cx| {
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
                                film.selected = crate::film_display::first_playable(&groups);
                                film.groups = groups;
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
        if !matches!(self.account, Account::SignedIn(_)) {
            return;
        }
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        let Some(session) = services.session.clone() else {
            return;
        };
        let id = self.id.clone();
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.spawn(async move {
            let mut events = session.lock().await.subscribe();
            loop {
                let saved = session.lock().await.is_watchlisted(&id);
                if sender.unbounded_send(saved).is_err() {
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
            while let Some(saved) = receiver.next().await {
                if entity
                    .update(cx, |film, cx| {
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
            self.previous_saved = self.saved;
            self.saved = saved;
            self.icon = Tween::fixed(0.0);
            self.icon.retarget(
                1.0,
                std::time::Duration::from_millis(300),
                Some(crate::theme::EASE_EDITORIAL),
                Instant::now(),
            );
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
                session
                    .lock()
                    .await
                    .set_watchlisted(&meta, saved)
                    .await
                    .is_ok()
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
