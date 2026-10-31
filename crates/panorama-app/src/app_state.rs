use crate::{
    addons_screen::model::{CatalogGroup, NAME_CAP, catalog_groups, display_text},
    catalog_switcher::{flatten, reconcile},
    services::{CatalogPage, Job, Services, ServicesHost, StartupError},
};
use futures::{StreamExt, channel::mpsc};
use gpui::{AppContext, Context, Task};
use panorama_core::{
    addons::{CatalogRef, FailureKind, Kind, ResourceEvent},
    stremio::{CoreChange, CoreErrorKind, Descriptor},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

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
    /// Catalog the user picked this session; `None` follows the saved or first one.
    pub remembered: Option<CatalogRef>,
    /// Home switcher groups in account order.
    catalog_groups: Vec<CatalogGroup>,
    /// Newest `select_catalog` save, so superseded ones are skipped.
    saves: Arc<AtomicU64>,
    /// Sanitized catalog failure.
    pub catalog_error: Option<String>,
    /// Whether the initial catalog is being requested.
    pub loading: bool,
    /// Whether an appended page is being requested.
    pub loading_more: bool,
    /// The single 160 MiB renderer cache shared across retained Home entries.
    pub images: gpui::Entity<crate::image_cache::ImageCache>,
    pub continue_watching: gpui::Entity<crate::continue_watching::ContinueWatching>,
    /// Last selected card, shown while Film metadata arrives.
    pub film_preview: Option<panorama_core::addons::FilmDetails>,
    /// The card a film page just shrank onto.
    pub quiet: Option<crate::zoom::Quiet>,
    /// Source lookups by film, reused between the hover prefetch and the film page.
    pub source_lookups: crate::source_lookup::SourceLookups,
    /// The film whose lookup feeds `streams`; a prefetch must not overwrite an open Player's.
    pub streams_for: Option<String>,
    /// Films warmed ahead of a click.
    pub prefetch: crate::prefetch::Prefetch,
    /// Stream groups for a film; Film keeps this current so an open Player follows late addons.
    pub streams: Option<(String, Arc<Vec<panorama_core::addons::StreamGroup>>)>,
    /// Source selected by Film for the next Player mount.
    pub playback: Option<crate::playback::selection::PlaybackSelection>,
    /// A play started from Home or Search, still looking up its sources for the open player.
    pub launch: Option<crate::playback::launch::Launch>,
    pub closing: bool,
    pub player: Option<crate::playback::shell::ShellPlayerState>,
    pub playback_host: Option<crate::playback::host::PlaybackHost>,
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
        let fixtures = services
            .as_ref()
            .is_some_and(|services| services.addons.is_none());
        let images = cx.new(|cx| crate::image_cache::ImageCache::new(fixtures, cx));
        let continue_watching =
            cx.new(|cx| crate::continue_watching::ContinueWatching::new(services.clone(), cx));
        let mut state = Self {
            images,
            continue_watching,
            film_preview: None,
            quiet: None,
            source_lookups: Default::default(),
            streams_for: None,
            prefetch: Default::default(),
            streams: None,
            playback: None,
            launch: None,
            playback_host: None,
            player: None,
            closing: false,
            account: Account::SignedOut,
            installed: vec![],
            services,
            runtime_error,
            catalog: None,
            remembered: None,
            catalog_groups: vec![],
            saves: Arc::new(AtomicU64::new(0)),
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
        if !more {
            self.refresh_groups();
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
        let (job, mut receiver) =
            services.catalog(self.installed.clone(), self.remembered.clone(), next, more);
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

    fn refresh_groups(&mut self) {
        self.catalog_groups = match self.services.as_ref().map(|s| s.addons.as_ref()) {
            None => vec![],
            Some(Some(client)) => {
                let names: Vec<_> = self
                    .installed
                    .iter()
                    .filter_map(|addon| {
                        let key = client.addon_key(addon).ok()?;
                        Some((key, display_text(&addon.summary().name, NAME_CAP)))
                    })
                    .collect();
                let mut catalogs = client.catalogs(&self.installed);
                catalogs.retain(|catalog| catalog.kind == Kind::Movie); // series: removed in the show page stage
                catalog_groups(&names, &catalogs)
            }
            Some(None) => crate::addons_screen::fixture::groups(),
        };
        let flat = flatten(&self.catalog_groups);
        self.remembered = reconcile(self.remembered.take(), &flat);
    }

    /// Catalog groups for the Home switcher; empty unless signed in.
    pub fn switcher_groups(&self) -> &[CatalogGroup] {
        if matches!(self.account, Account::SignedIn(_)) {
            &self.catalog_groups
        } else {
            &[]
        }
    }

    /// Show another catalog on Home and remember it for the next launch.
    pub fn select_catalog(&mut self, catalog: CatalogRef, cx: &mut Context<Self>) {
        if let Some(services) = &self.services
            && let Some(session) = services.session.clone()
        {
            // Saves queue on the session lock in call order; one that a newer pick
            // overtook is skipped, so the last pick is what stays saved.
            let saved = catalog.clone();
            let id = self.saves.fetch_add(1, Ordering::SeqCst) + 1;
            let latest = Arc::clone(&self.saves);
            services.runtime.spawn(async move {
                let mut session = session.lock().await;
                if latest.load(Ordering::SeqCst) != id {
                    return;
                }
                if let Err(error) = session.set_home_catalog(Some(saved)).await {
                    crate::debug::log(format!("home catalog not saved: {error:?}"));
                }
            });
        }
        self.remembered = Some(catalog);
        self.catalog = None;
        self.load(false, cx);
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
        self.remembered = None;
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
                            state.services = Some(services.clone());
                            state.runtime_error = None;
                            state
                                .continue_watching
                                .update(cx, |watching, cx| watching.restart(Some(services), cx));
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
