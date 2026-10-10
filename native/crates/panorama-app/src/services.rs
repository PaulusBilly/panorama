use futures::{StreamExt, channel::mpsc};
use panorama_core::{
    addons::{AddonClient, CatalogRef, CoreAddonTransport, FilmDetails, ResourceEvent},
    images::{ImageLoader, ImageLoaderOptions},
    store::{self, Store, StoreError},
    stremio::{
        CoreSession, Descriptor,
        env::{EnvConfig, PanoramaEnv},
    },
};
use std::sync::Arc;
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
    /// Executor retained by the process host.
    pub runtime: Handle,
    /// Persisted account session, used only on Tokio.
    pub session: Option<Arc<Mutex<CoreSession>>>,
    /// Cache-first addon client.
    pub addons: Option<AddonClient>,
    /// Decoding and disk image cache.
    pub images: Option<ImageLoader>,
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
            return Ok(Arc::new(Services {
                runtime: self.runtime.clone(),
                session: None,
                addons: None,
                images: None,
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
        let addons = AddonClient::new(store, CoreAddonTransport)
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
        Ok(Arc::new(Services {
            runtime: self.runtime.clone(),
            session: Some(Arc::new(Mutex::new(session))),
            addons: Some(addons),
            images: Some(images),
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
        next: Option<(CatalogRef, usize)>,
        fixture_next: bool,
    ) -> (Job<()>, mpsc::UnboundedReceiver<ResourceEvent<CatalogPage>>) {
        let (sender, receiver) = mpsc::unbounded();
        let services = self.clone();
        let job = self.runtime.spawn(async move {
            let Some(client) = &services.addons else {
                let mut films = crate::fixtures::films();
                if fixture_next {
                    for (index, film) in films.iter_mut().enumerate() {
                        film.id = format!("{}-next-{index}", film.id);
                    }
                }
                let _ = sender.unbounded_send(ResourceEvent::Fresh(CatalogPage {
                    skip: if fixture_next { 18 } else { 9 },
                    films,
                    source: None,
                    has_more: !fixture_next,
                }));
                return;
            };
            let mut stream = match next {
                Some((source, skip)) => client.catalog_next(&addons, source, skip),
                None => client.catalog(&addons, None),
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
}
