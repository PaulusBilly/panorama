use crate::services::{CatalogPage, Job, Services, ServicesHost, StartupError};
use futures::{StreamExt, channel::mpsc};
use gpui::{AppContext, Context, Task};
use panorama_core::{
    addons::{FailureKind, ResourceEvent},
    stremio::{CoreChange, CoreErrorKind, Descriptor},
};
use std::sync::Arc;

/// The account state presented to native screens.
#[derive(Clone, PartialEq, Eq)]
pub enum Account {
    /// No authenticated account.
    SignedOut,
    /// Sign-in is running on Tokio.
    Authenticating,
    /// Authenticated account, with display email only.
    SignedIn(String),
    /// Sanitized sign-in error copy.
    Error(String),
}

/// Film-to-Player handoff retaining the complete redacted source and behavior hints.
#[derive(Clone)]
pub struct PlaybackSelection {
    /// Opaque film identifier associated with the chosen source.
    pub film_id: String,
    /// First playable source in account group order.
    pub source: panorama_core::addons::StreamSource,
}

/// Map sanitized core failures to plain sign-in copy.
pub fn account_error(kind: CoreErrorKind) -> &'static str {
    match kind {
        CoreErrorKind::WrongCredentials => "Email or password is incorrect.",
        CoreErrorKind::Network => "Can't reach Stremio. Check your connection.",
        CoreErrorKind::Timeout => "Signing in took too long. Try again.",
        _ => "Something went wrong. Try again.",
    }
}

/// Account, installed addons and cache-first Home data shared by app entities.
pub struct AppState {
    /// Current account state.
    pub account: Account,
    /// Installed addons in account order.
    pub installed: Vec<Descriptor>,
    /// Live services, absent after a startup failure.
    pub services: Option<Arc<Services>>,
    /// Startup failure shown with Reload Panorama.
    pub runtime_error: Option<String>,
    /// Visible catalog, retained during refresh or failed network work.
    pub catalog: Option<CatalogPage>,
    /// Sanitized catalog failure.
    pub catalog_error: Option<String>,
    /// Whether the initial catalog is being requested.
    pub loading: bool,
    /// Whether an appended page is being requested.
    pub loading_more: bool,
    /// The single 160 MiB renderer cache shared across retained Home entries.
    pub images: gpui::Entity<crate::image_cache::ImageCache>,
    /// Last selected card, shown while Film metadata arrives.
    pub film_preview: Option<panorama_core::addons::FilmDetails>,
    /// Source selected by Film for the next Player mount.
    pub playback: Option<PlaybackSelection>,
    host: Arc<ServicesHost>,
    events: Option<Task<()>>,
    request: Option<Task<()>>,
    auth: Option<Task<()>>,
    reload: Option<Task<()>>,
}

impl AppState {
    /// Start GPUI consumers; all producers run on Tokio.
    pub fn new(
        host: Arc<ServicesHost>,
        initial: Result<Arc<Services>, StartupError>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (services, runtime_error) = match initial {
            Ok(services) => (Some(services), None),
            Err(StartupError::Locked) => (None, Some("Panorama is already running.".into())),
            Err(StartupError::Failed(message)) => (None, Some(message)),
        };
        let images = cx.new(crate::image_cache::ImageCache::new);
        let mut state = Self {
            images,
            film_preview: None,
            playback: None,
            account: Account::SignedOut,
            installed: vec![],
            services,
            runtime_error,
            catalog: None,
            catalog_error: None,
            loading: false,
            loading_more: false,
            host,
            events: None,
            request: None,
            auth: None,
            reload: None,
        };
        state.subscribe(cx);
        state.load(false, cx);
        state
    }

    fn subscribe(&mut self, cx: &mut Context<Self>) {
        let Some(services) = &self.services else {
            return;
        };
        let Some(session) = services.session.clone() else {
            self.account = Account::SignedIn("preview@panorama.local".into());
            return;
        };
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.clone().spawn(async move {
            let mut events = session.lock().await.subscribe();
            loop {
                let (account, addons) = {
                    let session = session.lock().await;
                    let profile = session.profile();
                    let account = profile
                        .as_core()
                        .auth
                        .as_ref()
                        .map_or(Account::SignedOut, |auth| {
                            Account::SignedIn(auth.user.email.clone())
                        });
                    (account, session.installed_addons())
                };
                if sender.unbounded_send((account, addons)).is_err() {
                    break;
                }
                loop {
                    match events.recv().await {
                        Ok(CoreChange::ProfileChanged | CoreChange::AddonsChanged)
                        | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        _ => {}
                    }
                }
            }
        }));
        self.events = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some((account, addons)) = receiver.next().await {
                if entity
                    .update(cx, |state, cx| {
                        let changed = state.installed != addons || state.account != account;
                        if !matches!(state.account, Account::Authenticating | Account::Error(_)) {
                            state.account = account;
                        }
                        state.installed = addons;
                        if changed
                            && !matches!(state.account, Account::Authenticating | Account::Error(_))
                        {
                            state.load(false, cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// Refresh or append a page, retaining the visible cached cards.
    pub fn load(&mut self, more: bool, cx: &mut Context<Self>) {
        if more && (self.loading_more || self.catalog.as_ref().is_none_or(|p| !p.has_more)) {
            return;
        }
        let Some(services) = &self.services else {
            return;
        };
        let next = if more {
            self.catalog
                .as_ref()
                .and_then(|page| page.source.clone().map(|source| (source, page.skip)))
        } else {
            None
        };
        let (job, mut receiver) = services.catalog(self.installed.clone(), next, more);
        self.loading = !more;
        self.loading_more = more;
        self.catalog_error = None;
        self.request = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some(event) = receiver.next().await {
                if entity
                    .update(cx, |state, cx| {
                        match event {
                            ResourceEvent::CacheHit(page) | ResourceEvent::Fresh(page) => {
                                if more {
                                    if let Some(current) = &mut state.catalog {
                                        for film in page.films {
                                            if !current.films.iter().any(|f| f.id == film.id) {
                                                current.films.push(film);
                                            }
                                        }
                                        current.skip = page.skip;
                                        current.has_more = page.has_more;
                                    }
                                } else {
                                    state.catalog = Some(page);
                                }
                                state.loading = false;
                                state.loading_more = false;
                            }
                            ResourceEvent::Failed(kind) => {
                                state.catalog_error = Some(
                                    match kind {
                                        FailureKind::Offline => {
                                            "Can't reach your film catalog. Check your connection."
                                        }
                                        FailureKind::Timeout => {
                                            "Loading films took too long. Try again."
                                        }
                                        _ => "Could not load popular films. Try again.",
                                    }
                                    .into(),
                                );
                                state.loading = false;
                                state.loading_more = false;
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

    /// Authenticate without retaining or logging credentials in the app state.
    pub fn sign_in(&mut self, email: String, password: String, cx: &mut Context<Self>) {
        if matches!(self.account, Account::Authenticating) {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        self.account = Account::Authenticating;
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.clone().spawn(async move {
            let result = if let Some(session) = &services.session {
                let mut session = session.lock().await;
                session
                    .sign_in(email, password)
                    .await
                    .map(|profile| {
                        (
                            profile
                                .as_core()
                                .auth
                                .as_ref()
                                .map(|auth| auth.user.email.clone())
                                .unwrap_or_default(),
                            session.installed_addons(),
                        )
                    })
                    .map_err(|error| error.kind)
            } else {
                drop(password);
                Ok((email, vec![]))
            };
            let _ = sender.unbounded_send(result);
        }));
        self.auth = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            if let Some(result) = receiver.next().await {
                let _ = entity.update(cx, |state, cx| {
                    match result {
                        Ok((email, addons)) => {
                            state.account = Account::SignedIn(email);
                            state.installed = addons;
                            state.load(false, cx);
                        }
                        Err(kind) => state.account = Account::Error(account_error(kind).into()),
                    }
                    cx.notify();
                });
            }
        }));
        cx.notify();
    }

    /// Cancel a pending sign-in when its dialog closes.
    pub fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        if matches!(self.account, Account::Authenticating | Account::Error(_)) {
            self.auth = None;
            self.account = Account::SignedOut;
            cx.notify();
        }
    }

    /// Drop catalog requests before signing out, then fetch signed-out catalogs.
    pub fn sign_out(&mut self, cx: &mut Context<Self>) {
        self.request = None;
        self.catalog = None;
        self.installed.clear();
        self.account = Account::SignedOut;
        let Some(services) = self.services.clone() else {
            return;
        };
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.clone().spawn(async move {
            let result = if let Some(session) = &services.session {
                session.lock().await.sign_out().await.map_err(|e| e.kind)
            } else {
                Ok(())
            };
            let _ = sender.unbounded_send(result);
        }));
        self.auth = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            if let Some(result) = receiver.next().await {
                let _ = entity.update(cx, |state, cx| {
                    if result.is_err() {
                        state.runtime_error = Some(
                            "Could not finish signing out. Reload Panorama to try again.".into(),
                        );
                    }
                    state.load(false, cx);
                });
            }
        }));
        cx.notify();
    }

    /// Restart failed services on Tokio, preserving the installed Env on retry.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.reload.is_some() {
            return;
        }
        self.events = None;
        self.request = None;
        self.auth = None;
        self.services = None;
        let host = self.host.clone();
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(self.host.runtime.spawn(async move {
            let _ = sender.unbounded_send(host.start().await);
        }));
        self.reload = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            if let Some(result) = receiver.next().await {
                let _ = entity.update(cx, |state, cx| {
                    state.reload = None;
                    match result {
                        Ok(services) => {
                            state.services = Some(services);
                            state.runtime_error = None;
                            state.subscribe(cx);
                            state.load(false, cx);
                        }
                        Err(StartupError::Locked) => {
                            state.runtime_error = Some("Panorama is already running.".into())
                        }
                        Err(StartupError::Failed(message)) => state.runtime_error = Some(message),
                    }
                    cx.notify();
                });
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_failures_never_expose_core_diagnostics() {
        for (kind, copy) in [
            (
                CoreErrorKind::WrongCredentials,
                "Email or password is incorrect.",
            ),
            (
                CoreErrorKind::Network,
                "Can't reach Stremio. Check your connection.",
            ),
            (
                CoreErrorKind::Timeout,
                "Signing in took too long. Try again.",
            ),
            (CoreErrorKind::Storage, "Something went wrong. Try again."),
            (
                CoreErrorKind::AlreadyInstalled,
                "Something went wrong. Try again.",
            ),
            (
                CoreErrorKind::Environment,
                "Something went wrong. Try again.",
            ),
            (CoreErrorKind::Other, "Something went wrong. Try again."),
        ] {
            assert_eq!(account_error(kind), copy);
        }
    }
}
