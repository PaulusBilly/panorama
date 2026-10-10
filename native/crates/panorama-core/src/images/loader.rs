use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use futures::{
    FutureExt, StreamExt,
    future::{BoxFuture, Shared},
};
use image::DynamicImage;
use tokio::sync::Semaphore;

use super::{
    DecodedImage, ImageError, ImageLoaderOptions, ImageRequest, ImageUrl, cache::Cache, decode,
    valid_url,
};

type Flight = Shared<BoxFuture<'static, Result<Arc<DynamicImage>, ImageError>>>;

/// Shareable loader. Concurrent requests for a URL share acquisition and decoding.
/// Dropping the last interested load future cancels a pending download. Blocking
/// work already running can finish, but clearing prevents older loads reinserting.
#[derive(Clone)]
pub struct ImageLoader {
    inner: Arc<Inner>,
    flights: Arc<Mutex<HashMap<ImageUrl, Weak<Flight>>>>,
}

struct Inner {
    cache: Arc<Cache>,
    client: reqwest::Client,
    downloads: Semaphore,
    options: ImageLoaderOptions,
    #[cfg(test)]
    before_read: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ImageLoader {
    /// Opens/rebuilds the cache and constructs an HTTPS-only, rustls HTTP client.
    /// Call off the UI thread; subsequent `load` and `clear` operations are async.
    pub fn new(options: ImageLoaderOptions) -> Result<Self, ImageError> {
        let client = client_builder().build().map_err(|_| ImageError::Network)?;
        Self::from_client(options, client)
    }

    #[cfg(test)]
    pub(super) fn with_client(
        options: ImageLoaderOptions,
        client: reqwest::Client,
    ) -> Result<Self, ImageError> {
        Self::from_client(options, client)
    }

    fn from_client(
        options: ImageLoaderOptions,
        client: reqwest::Client,
    ) -> Result<Self, ImageError> {
        if options.max_concurrent == 0 || options.max_concurrent > Semaphore::MAX_PERMITS {
            return Err(ImageError::Cache);
        }
        let cache = Arc::new(Cache::new(&options)?);
        Ok(Self {
            inner: Arc::new(Inner {
                cache,
                client,
                downloads: Semaphore::new(options.max_concurrent),
                options,
                #[cfg(test)]
                before_read: None,
            }),
            flights: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    #[cfg(test)]
    pub(super) fn before_read(&mut self, hook: Arc<dyn Fn() + Send + Sync>) {
        Arc::get_mut(&mut self.inner).unwrap().before_read = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn on_blocked_lock(&self, hook: Box<dyn FnOnce() + Send>) {
        self.inner.cache.on_blocked_lock(hook);
    }

    /// Loads from disk first, otherwise downloads, verifies, and caches originals.
    /// Decoding and aspect-preserving downscaling always run on blocking workers.
    pub async fn load(&self, request: ImageRequest) -> Result<DecodedImage, ImageError> {
        if request.target.max_width == 0 || request.target.max_height == 0 {
            return Err(ImageError::Decode);
        }
        let flight = {
            let mut flights = self.flights.lock().unwrap_or_else(|e| e.into_inner());
            flights.retain(|_, flight| flight.strong_count() != 0);
            if let Some(flight) = flights.get(&request.url).and_then(Weak::upgrade) {
                flight
            } else {
                let inner = Arc::clone(&self.inner);
                let local_generation = inner.cache.local_generation();
                let url = request.url.clone();
                let flight = Arc::new(
                    async move { inner.acquire(url, local_generation).await }
                        .boxed()
                        .shared(),
                );
                flights.insert(request.url, Arc::downgrade(&flight));
                flight
            }
        };
        let image = flight.as_ref().clone().await?;
        tokio::task::spawn_blocking(move || decode::resize(image, request.target))
            .await
            .map_err(|_| ImageError::Decode)
    }

    /// Removes cached files and invalidates inserts from loads started before clear.
    /// The sibling coordination file is retained; the cache directory is emptied.
    pub async fn clear(&self) -> Result<(), ImageError> {
        {
            let mut flights = self.flights.lock().unwrap_or_else(|e| e.into_inner());
            self.inner.cache.invalidate();
            flights.clear();
        }
        let cache = Arc::clone(&self.inner.cache);
        tokio::task::spawn_blocking(move || cache.clear())
            .await
            .map_err(|_| ImageError::Cache)?
    }
}

impl Inner {
    async fn acquire(
        &self,
        url: ImageUrl,
        local_generation: u64,
    ) -> Result<Arc<DynamicImage>, ImageError> {
        let key = Cache::key(&url);
        let cache = Arc::clone(&self.cache);
        let read_key = key.clone();
        #[cfg(test)]
        let before_read = self.before_read.clone();
        let (cached, generation) = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            if let Some(hook) = before_read {
                hook();
            }
            cache.read(&read_key)
        })
        .await
        .map_err(|_| ImageError::Cache)??;
        if let Some(image) = cached {
            return Ok(Arc::new(image));
        }
        let bytes = {
            let _permit = self
                .downloads
                .acquire()
                .await
                .map_err(|_| ImageError::Network)?;
            tokio::time::timeout(self.options.timeout, self.fetch(url))
                .await
                .map_err(|_| ImageError::Timeout)??
        };
        let cache = Arc::clone(&self.cache);
        tokio::task::spawn_blocking(move || {
            let image = decode::decode(&bytes)?;
            cache.insert(&key, &bytes, generation, local_generation)?;
            Ok(Arc::new(image))
        })
        .await
        .map_err(|_| ImageError::Decode)?
    }

    async fn fetch(&self, url: ImageUrl) -> Result<Vec<u8>, ImageError> {
        let response = self.client.get(url.0).send().await.map_err(network_error)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(ImageError::Network);
        }
        let cap = self.options.max_body_bytes;
        if response.content_length().is_some_and(|size| size > cap) {
            return Err(ImageError::TooLarge);
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(network_error)?;
            if (bytes.len() as u64).saturating_add(chunk.len() as u64) > cap {
                return Err(ImageError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

pub(super) fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .no_proxy()
        // Image URLs can carry addon tokens; never forward them to a redirect target.
        .referer(false)
        .dns_resolver(Arc::new(super::destination::SafeDns))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if !valid_url(attempt.url()) || attempt.previous().len() > 5 {
                attempt.error("image redirect rejected")
            } else {
                attempt.follow()
            }
        }))
}

fn network_error(error: reqwest::Error) -> ImageError {
    if error.is_timeout() {
        ImageError::Timeout
    } else {
        ImageError::Network
    }
}
