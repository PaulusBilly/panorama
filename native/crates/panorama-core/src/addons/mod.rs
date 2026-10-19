//! Cache-first addon data for Home, Search and Film, with cancellable request streams.
//! Pass account-ordered descriptors on every call, or an empty slice when signed out.
//! Construction is async because installation salt storage runs on a blocking worker.

mod catalog;
mod details;
pub mod manage;
mod sanitize;
mod search;
mod transport;
mod types;

pub use transport::{AddonTransport, CoreAddonTransport, TransportResponse};
pub use types::*;

pub(crate) use transport::capture_metadata;

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures::{Stream, StreamExt, channel::mpsc, future::BoxFuture};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use stremio_core::{
    constants::CINEMETA_URL,
    types::addon::{Descriptor as CoreDescriptor, ManifestResource, ResourcePath, ResourceRequest},
};
use tokio::task::JoinHandle;

use crate::{
    store::{Key, Store},
    stremio::Descriptor,
};

/// Stateless addon client; each request receives the current installation list.
#[derive(Clone)]
pub struct AddonClient {
    store: Arc<Store>,
    transport: Arc<dyn AddonTransport>,
    salt: [u8; 32],
    runtime: tokio::runtime::Handle,
    generation: Option<u64>,
}

#[derive(Clone)]
struct Installation {
    key: AddonKey,
    descriptor: Arc<CoreDescriptor>,
}

fn supports(addon: &Installation, path: &ResourcePath, ignore_prefix: bool) -> bool {
    let manifest = &addon.descriptor.manifest;
    manifest.resources.iter().any(|resource| {
        let (name, types, prefixes) = match resource {
            ManifestResource::Short(name) => (name, &manifest.types, manifest.id_prefixes.as_ref()),
            ManifestResource::Full {
                name,
                types,
                id_prefixes,
            } => (
                name,
                types.as_ref().unwrap_or(&manifest.types),
                id_prefixes.as_ref().or(manifest.id_prefixes.as_ref()),
            ),
        };
        name == &path.resource
            && types.contains(&path.r#type)
            && (ignore_prefix
                || prefixes.is_none_or(|prefixes| {
                    prefixes.is_empty() || prefixes.iter().any(|prefix| path.id.starts_with(prefix))
                }))
    })
}

struct EventReceiver<T> {
    receiver: mpsc::Receiver<T>,
    task: JoinHandle<()>,
}

impl<T: Unpin> Stream for EventReceiver<T> {
    type Item = T;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.receiver.poll_next_unpin(cx)
    }
}

impl<T> Drop for EventReceiver<T> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn delivery<T: Send + Unpin + 'static>(
    runtime: &tokio::runtime::Handle,
    work: impl FnOnce(mpsc::Sender<T>) -> BoxFuture<'static, ()>,
) -> Pin<Box<dyn Stream<Item = T> + Send>> {
    let (sender, receiver) = mpsc::channel(1);
    let task = runtime.spawn(work(sender));
    Box::pin(EventReceiver { receiver, task })
}

impl AddonClient {
    /// Creates the client, atomically generating a persisted 32-byte random salt.
    /// The salt survives sign-out as a preference and contains no account or URL data.
    /// Await construction on Tokio; subsequent screen calls may run on any thread.
    pub async fn new(
        store: Arc<Store>,
        transport: impl AddonTransport,
    ) -> Result<Self, FailureKind> {
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|_| FailureKind::InvalidInput)?;
        let storage = Arc::clone(&store);
        let salt = tokio::task::spawn_blocking(move || {
            let mut fresh = [0u8; 32];
            getrandom::fill(&mut fresh).map_err(|_| FailureKind::Storage)?;
            let key = Key::pref("installSalt").map_err(|_| FailureKind::Storage)?;
            let saved = storage
                .get_or_insert(&key, &fresh)
                .map_err(|_| FailureKind::Storage)?;
            saved.try_into().map_err(|_| FailureKind::Storage)
        })
        .await
        .map_err(|_| FailureKind::Storage)??;
        Ok(Self {
            store,
            transport: Arc::new(transport),
            salt,
            runtime,
            generation: None,
        })
    }

    /// Returns an installation's opaque identity without exposing its transport URL.
    pub fn addon_key(&self, addon: &Descriptor) -> Result<AddonKey, FailureKind> {
        self.identity(addon.as_core())
    }

    fn for_request(&self) -> Self {
        let mut client = self.clone();
        client.generation = Some(
            self.generation
                .unwrap_or_else(|| self.store.cache_generation()),
        );
        client
    }

    fn identity(&self, addon: &CoreDescriptor) -> Result<AddonKey, FailureKind> {
        let id = &addon.manifest.id;
        if id.is_empty()
            || id.len() > 450
            || id.chars().any(|c| {
                c.is_control()
                    || matches!(c, '/' | ':' | '|' | '#' | '?' | '\\')
                    || c.is_whitespace()
            })
        {
            return Err(FailureKind::InvalidInput);
        }
        let mut hash = Sha256::new();
        hash.update(self.salt);
        hash.update(addon.transport_url.as_str().as_bytes());
        let digest = hash.finalize();
        let suffix = digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(AddonKey(format!("{id}#{suffix}")))
    }

    fn installations<'a>(
        &'a self,
        addons: &'a [Descriptor],
    ) -> impl Iterator<Item = Installation> + 'a {
        addons.iter().filter_map(|addon| {
            self.addon_key(addon).ok().map(|key| Installation {
                key,
                descriptor: addon.shared_core(),
            })
        })
    }

    fn cinemeta(&self) -> Installation {
        let descriptor = CoreDescriptor {
            transport_url: CINEMETA_URL.to_owned(),
            manifest: stremio_core::types::addon::Manifest {
                id: "com.linvo.cinemeta".into(),
                version: stremio_core::types::addon::Version::new(3, 0, 0),
                name: "Cinemeta".into(),
                description: None,
                logo: None,
                background: None,
                contact_email: None,
                types: vec!["movie".into()],
                resources: vec!["meta".into()],
                id_prefixes: Some(vec!["tt".into()]),
                catalogs: vec![],
                addon_catalogs: vec![],
                behavior_hints: Default::default(),
            },
            flags: Default::default(),
        };
        let mut hash = Sha256::new();
        hash.update(self.salt);
        hash.update(CINEMETA_URL.as_str().as_bytes());
        let suffix = hash.finalize()[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Installation {
            key: AddonKey(format!("com.linvo.cinemeta#{suffix}")),
            descriptor: Arc::new(descriptor),
        }
    }

    async fn request(
        &self,
        addon: &Installation,
        path: ResourcePath,
        seconds: u64,
    ) -> Result<sanitize::Response, FailureKind> {
        let request = ResourceRequest::new(addon.descriptor.transport_url.clone(), path);
        let response = tokio::time::timeout(
            Duration::from_secs(seconds),
            self.transport.resource(request),
        )
        .await
        .map_err(|_| FailureKind::Timeout)??;
        sanitize::parse(response)
    }

    async fn read<T: DeserializeOwned + Send + 'static>(
        &self,
        key: Key,
    ) -> Result<Option<T>, FailureKind> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            let bytes = store.get(&key).map_err(|_| FailureKind::Storage)?;
            Ok(bytes.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
        })
        .await
        .map_err(|_| FailureKind::Storage)?
    }

    async fn write<T: Serialize>(&self, key: Key, value: &T) -> Result<(), FailureKind> {
        let bytes = serde_json::to_vec(value).map_err(|_| FailureKind::Storage)?;
        let store = Arc::clone(&self.store);
        let generation = self.generation.unwrap_or_else(|| store.cache_generation());
        tokio::task::spawn_blocking(move || {
            store
                .set_if_generation(&key, &bytes, generation)
                .map_err(|_| FailureKind::Storage)
        })
        .await
        .map_err(|_| FailureKind::Storage)?
    }
}

#[cfg(test)]
mod tests;
