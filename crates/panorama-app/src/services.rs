use futures::{StreamExt, channel::mpsc};
use panorama_core::{
    addons::{AddonClient, CatalogRef, CoreAddonTransport, FilmDetails, Kind, ResourceEvent},
    images::{ImageLoader, ImageLoaderOptions, ImageRequest, ImageTarget, ImageUrl},
    store::{self, Store, StoreError},
    stremio::{
        ContinueItem, CoreChange, CoreSession, Descriptor, ResumePoint,
        env::{EnvConfig, PanoramaEnv},
    },
};
use std::sync::Arc;
#[path = "services_search.rs"]
mod search;
use tokio::{
    runtime::{Handle, Runtime},
    sync::Mutex,
    task::JoinHandle,
};

/// Startup failures with a distinct single-instance state.
#[derive(Clone, Debug)]
pub enum StartupError {
    /// Another process owns the store.
    Locked,
    /// A sanitized, retryable startup failure.
    Failed(String),
}

/// Creates the I/O runtime before the first GPUI window.
pub fn runtime() -> Result<Runtime, StartupError> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("panorama-io")
        .enable_all()
        .build()
        .map_err(|_| StartupError::Failed("Could not start Panorama's background services.".into()))
}

/// Shared startup resources, retained across retries.
pub struct ServicesHost {
    /// Executor for every core operation.
    pub runtime: Handle,
    fixtures: bool,
    store: Mutex<Option<Arc<Store>>>,
}

/// Live services, or in-process fixtures with no external I/O.
pub struct Services {
    pub data_dir: std::path::PathBuf,
    pub discord: Option<panorama_core::discord::DiscordPresence>,
    /// Executor retained by the process host.
    pub runtime: Handle,
    /// Persisted account session, used only on Tokio.
    pub session: Option<Arc<Mutex<CoreSession>>>,
    /// Cache-first addon client.
    pub addons: Option<AddonClient>,
    /// Decoding and disk image cache.
    pub images: Option<ImageLoader>,
    /// The single store; absent in fixture mode so previews never touch saved preferences.
    pub store: Option<Arc<Store>>,
    /// Ordered preference writes; present exactly when `store` is.
    pub prefs_writer: Option<crate::playback::prefs::PrefsWriter>,
    /// The fixture presence's opt-in file, removed with the services.
    pub fixture_file: Option<FixtureFile>,
    /// Wikidata awards for the film page; absent in fixture mode.
    pub credits: Option<panorama_core::credits::CreditsClient>,
    /// MUBI awards, festivals and festival pages; absent in fixture mode.
    pub mubi: Option<panorama_core::mubi::MubiClient>,
}

/// A per-run temporary file removed on drop.
pub struct FixtureFile(std::path::PathBuf);

impl FixtureFile {
    /// A unique path in the temp directory (process id and a clock suffix).
    pub fn unique() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        Self(std::env::temp_dir().join(format!(
            "panorama-fixture-discord-{}-{nanos:x}.json",
            std::process::id()
        )))
    }

    /// The path.
    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for FixtureFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl ServicesHost {
    /// Construct a retryable host without touching disk.
    pub fn new(runtime: Handle, fixtures: bool) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            fixtures,
            store: Mutex::new(None),
        })
    }

    /// Open storage and initialize core services on Tokio and blocking workers.
    pub async fn start(&self) -> Result<Arc<Services>, StartupError> {
        if self.fixtures {
            let fixture_file = FixtureFile::unique();
            return Ok(Arc::new(Services {
                runtime: self.runtime.clone(),
                data_dir: store::default_path()
                    .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
                    .unwrap_or_default(),
                // A presence with no IPC paths: the toggle works, nothing connects, and the
                // opt-in lands in a temporary file instead of the user's settings.
                discord: Some(panorama_core::discord::DiscordPresence::new(
                    &self.runtime,
                    fixture_file.path(),
                    vec![],
                    panorama_core::discord::SystemConnector,
                )),
                session: None,
                addons: None,
                images: None,
                store: None,
                prefs_writer: None,
                fixture_file: Some(fixture_file),
                credits: None,
                mubi: None,
            }));
        }
        let path = store::default_path().ok_or_else(|| {
            StartupError::Failed("Could not locate Panorama's storage directory.".into())
        })?;
        let mut slot = self.store.lock().await;
        if slot.is_none() {
            let store_path = path.clone();
            let store = tokio::task::spawn_blocking(move || Store::open(&store_path))
                .await
                .map_err(|_| StartupError::Failed("Could not open Panorama's storage.".into()))?
                .map_err(|error| match error {
                    StoreError::Locked => StartupError::Locked,
                    _ => StartupError::Failed("Could not open Panorama's storage.".into()),
                })?
                .0;
            let store = Arc::new(store);
            PanoramaEnv::install(EnvConfig {
                store: store.clone(),
                api_base: Some(
                    url::Url::parse("https://api.strem.io")
                        .map_err(|_| StartupError::Failed("Could not configure Stremio.".into()))?,
                ),
                runtime: self.runtime.clone(),
            })
            .map_err(|_| StartupError::Failed("Could not initialize Stremio.".into()))?;
            *slot = Some(store);
        }
        let store = slot
            .as_ref()
            .cloned()
            .ok_or_else(|| StartupError::Failed("Storage is unavailable.".into()))?;
        let session = CoreSession::start()
            .await
            .map_err(|_| StartupError::Failed("Could not restore your Stremio account.".into()))?;
        let addons = AddonClient::new(store.clone(), CoreAddonTransport)
            .await
            .map_err(|_| StartupError::Failed("Could not initialize addons.".into()))?;
        let cache_dir = path
            .parent()
            .ok_or_else(|| StartupError::Failed("Could not locate the image cache.".into()))?
            .join("cache")
            .join("images");
        let images = tokio::task::spawn_blocking(move || {
            ImageLoader::new(ImageLoaderOptions {
                cache_dir,
                max_disk_bytes: 256 * 1024 * 1024,
                ..Default::default()
            })
        })
        .await
        .map_err(|_| StartupError::Failed("Could not initialize the image cache.".into()))?
        .map_err(|_| StartupError::Failed("Could not initialize the image cache.".into()))?;
        let data_dir = path
            .parent()
            .ok_or_else(|| StartupError::Failed("Could not locate playback storage.".into()))?
            .to_path_buf();
        let discord = panorama_core::discord::DiscordPresence::new(
            &self.runtime,
            data_dir.join("discord-presence.json"),
            panorama_core::discord::discord_paths(
                panorama_core::discord::Platform::current(),
                |key| std::env::var(key).ok(),
            ),
            panorama_core::discord::SystemConnector,
        );
        Ok(Arc::new(Services {
            data_dir,
            discord: Some(discord),
            runtime: self.runtime.clone(),
            session: Some(Arc::new(Mutex::new(session))),
            addons: Some(addons),
            images: Some(images),
            prefs_writer: Some(
                crate::playback::prefs::PrefsWriter::spawn(&self.runtime, store.clone()).0,
            ),
            store: Some(store),
            fixture_file: None,
            credits: panorama_core::credits::CreditsClient::new().ok(),
            mubi: panorama_core::mubi::MubiClient::new().ok(),
        }))
    }
}

/// Screen-facing page including its paging cursor.
#[derive(Clone)]
pub struct CatalogPage {
    /// Film cards in source order.
    pub films: Vec<FilmDetails>,
    /// Actual source; fixture pages have no addon source.
    pub source: Option<CatalogRef>,
    /// Next raw source offset.
    pub skip: usize,
    /// Whether another page is available.
    pub has_more: bool,
}

/// A Tokio job whose owner cancels it by dropping the bridge.
pub struct Job<T>(pub JoinHandle<T>);
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Services {
    /// Bridge cache-first catalog events without polling core on GPUI.
    pub fn catalog(
        self: &Arc<Self>,
        addons: Vec<Descriptor>,
        remembered: Option<CatalogRef>,
        next: Option<(CatalogRef, usize)>,
        fixture_next: bool,
    ) -> (Job<()>, mpsc::UnboundedReceiver<ResourceEvent<CatalogPage>>) {
        let (sender, receiver) = mpsc::unbounded();
        let services = self.clone();
        let job = self.runtime.spawn(async move {
            let Some(client) = &services.addons else {
                let catalogs = crate::addons_screen::fixture::catalogs();
                let chosen = remembered
                    .and_then(|chosen| {
                        catalogs
                            .iter()
                            .position(|catalog| catalog.catalog_id == chosen.catalog_id)
                    })
                    .unwrap_or(0);
                let mut films = crate::fixtures::films();
                films.rotate_left(chosen);
                if fixture_next {
                    for (index, film) in films.iter_mut().enumerate() {
                        film.id = format!("{}-next-{index}", film.id);
                    }
                }
                let _ = sender.unbounded_send(ResourceEvent::Fresh(CatalogPage {
                    skip: if fixture_next { 18 } else { 9 },
                    films,
                    source: catalogs.get(chosen).cloned(),
                    has_more: !fixture_next,
                }));
                return;
            };
            let mut stream = match next {
                Some((source, skip)) => client.catalog_next(&addons, source, skip),
                None => {
                    let remembered = match (remembered, &services.session) {
                        (Some(chosen), _) => Some(chosen),
                        (None, Some(session)) if !addons.is_empty() => {
                            session.lock().await.home_catalog().await.ok().flatten()
                        }
                        _ => None,
                    };
                    client.catalog(&addons, remembered)
                }
            };
            while let Some(event) = stream.next().await {
                let event = match event {
                    ResourceEvent::CacheHit(page) => ResourceEvent::CacheHit(CatalogPage {
                        films: page.items,
                        source: Some(page.catalog),
                        skip: page.next_skip,
                        has_more: page.has_more,
                    }),
                    ResourceEvent::Fresh(page) => ResourceEvent::Fresh(CatalogPage {
                        films: page.items,
                        source: Some(page.catalog),
                        skip: page.next_skip,
                        has_more: page.has_more,
                    }),
                    ResourceEvent::Failed(kind) => ResourceEvent::Failed(kind),
                };
                if sender.unbounded_send(event).is_err() {
                    break;
                }
            }
        });
        (Job(job), receiver)
    }

    /// Films in progress: a snapshot on start, then again on every library or profile change.
    /// Fixtures send their list once.
    pub fn continue_watching(
        self: &Arc<Self>,
    ) -> (Job<()>, mpsc::UnboundedReceiver<Vec<ContinueItem>>) {
        let (sender, receiver) = mpsc::unbounded();
        let services = self.clone();
        let job = self.runtime.spawn(async move {
            let Some(session) = services.session.clone() else {
                let _ = sender.unbounded_send(crate::fixtures::continue_items());
                return;
            };
            let mut events = session.lock().await.subscribe();
            loop {
                let items = session.lock().await.continue_watching();
                if sender.unbounded_send(items).is_err() {
                    break;
                }
                if !wait_for_library_change(&mut events).await {
                    break;
                }
            }
        });
        (Job(job), receiver)
    }

    /// Addon details for one film, using the session's installed addons. Fixtures answer from
    /// `fixtures::details`.
    pub fn details(
        self: &Arc<Self>,
        id: String,
    ) -> (Job<()>, mpsc::UnboundedReceiver<ResourceEvent<FilmDetails>>) {
        let (sender, receiver) = mpsc::unbounded();
        let services = self.clone();
        let job = self.runtime.spawn(async move {
            let (Some(client), Some(session)) = (&services.addons, &services.session) else {
                let event = crate::fixtures::details(&id).map_or(
                    ResourceEvent::Failed(panorama_core::addons::FailureKind::InvalidInput),
                    ResourceEvent::Fresh,
                );
                let _ = sender.unbounded_send(event);
                return;
            };
            let addons = session.lock().await.installed_addons();
            let mut events = client.details(&addons, Kind::Movie, &id, None);
            while let Some(event) = events.next().await {
                if sender.unbounded_send(event).is_err() {
                    break;
                }
            }
        });
        (Job(job), receiver)
    }

    /// Bring a film's backdrop and logo into the disk cache. The small target skips keeping a
    /// full-size decode; the page decodes the cached original at its own size.
    pub fn warm_artwork(self: &Arc<Self>, film: &FilmDetails) -> Job<()> {
        let loader = self.images.clone();
        let parse = |url: Option<&url::Url>| ImageUrl::parse(url?.as_str()).ok();
        let banner = parse(film.background.as_ref().or(film.poster.as_ref()));
        // The page's slot is unknown here; a logo always fits w780 and a banner may not.
        let logo = parse(film.logo.as_ref()).map(|url| url.sized_for(768));
        let requests = [banner, logo]
            .into_iter()
            .flatten()
            .map(|url| ImageRequest {
                url,
                target: ImageTarget {
                    max_width: 64,
                    max_height: 64,
                },
            })
            .collect::<Vec<_>>();
        Job(self.runtime.spawn(async move {
            if let Some(loader) = loader {
                futures::future::join_all(requests.into_iter().map(|request| loader.load(request)))
                    .await;
            }
        }))
    }

    /// A film's foreign title. Addons rarely send one, so it comes from Wikidata, once per film:
    /// the answer, even none, is kept in the store. A network failure is retried next visit.
    pub fn original_title(self: &Arc<Self>, id: String) -> Job<Option<String>> {
        let (wikidata, store) = (self.credits.clone(), self.store.clone());
        Job(self.runtime.spawn(async move {
            let wikidata = wikidata?;
            let key = panorama_core::store::Key::original_title(&id).ok();
            let cached = store
                .as_ref()
                .zip(key.as_ref())
                .and_then(|(store, key)| store.get(key).ok().flatten())
                .and_then(|bytes| serde_json::from_slice::<Option<String>>(&bytes).ok());
            if let Some(title) = cached {
                return title;
            }
            let title = wikidata.original_title(&id).await;
            if let (Ok(title), Some(store), Some(key)) = (&title, &store, &key)
                && let Ok(bytes) = serde_json::to_vec(title)
            {
                let _ = store.set(key, &bytes);
            }
            title.ok().flatten()
        }))
    }

    /// The addon transport that supplied a film's cached metadata, for the playback target.
    pub async fn metadata_base(&self, id: &str) -> Option<url::Url> {
        let (client, session) = (self.addons.as_ref()?, self.session.as_ref()?);
        let addons = session.lock().await.installed_addons();
        client.metadata_base(&addons, id).await
    }

    /// Whether a film is saved and where to resume it, now and after every library change.
    /// Sends nothing without a session.
    pub fn library_flags(
        self: &Arc<Self>,
        id: String,
    ) -> (
        Job<()>,
        mpsc::UnboundedReceiver<(bool, Option<ResumePoint>)>,
    ) {
        let (sender, receiver) = mpsc::unbounded();
        let session = self.session.clone();
        let job = self.runtime.spawn(async move {
            let Some(session) = session else {
                return;
            };
            let mut events = session.lock().await.subscribe();
            loop {
                let flags = {
                    let session = session.lock().await;
                    (session.in_library(&id), session.resume(&id))
                };
                if sender.unbounded_send(flags).is_err() {
                    break;
                }
                if !wait_for_library_change(&mut events).await {
                    break;
                }
            }
        });
        (Job(job), receiver)
    }
}

/// Waits for the next change that can alter library state; false once the session is gone.
async fn wait_for_library_change(
    events: &mut tokio::sync::broadcast::Receiver<CoreChange>,
) -> bool {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        match events.recv().await {
            Ok(CoreChange::LibraryChanged | CoreChange::ProfileChanged)
            | Err(RecvError::Lagged(_)) => return true,
            Err(RecvError::Closed) => return false,
            _ => {}
        }
    }
}
