use std::sync::Arc;

use serde::{Serialize, de::DeserializeOwned};
use stremio_core::types::addon::ManifestResource;

use super::{CoreSession, Descriptor, env::State};
use crate::{
    addons::{AddonClient, AddonKey, CatalogRef, manage::InstallError},
    store::Key,
};

async fn read<T: DeserializeOwned>(
    state: &Arc<State>,
    name: &str,
) -> Result<Option<T>, InstallError> {
    let key = Key::pref(name).map_err(|_| InstallError::Storage)?;
    let bytes = state
        .blocking(move |store| store.get(&key))
        .await
        .map_err(|_| InstallError::Storage)?;
    Ok(bytes.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

async fn write<T: Serialize>(
    state: &Arc<State>,
    name: &str,
    value: Option<T>,
) -> Result<(), InstallError> {
    let key = Key::pref(name).map_err(|_| InstallError::Storage)?;
    let bytes = value
        .map(|value| serde_json::to_vec(&value))
        .transpose()
        .map_err(|_| InstallError::Storage)?;
    state
        .blocking(move |store| match bytes {
            Some(bytes) => store.set(&key, &bytes),
            None => store.remove(&key),
        })
        .await
        .map_err(|_| InstallError::Storage)
}

fn valid_meta(client: &AddonClient, addons: &[Descriptor], key: &AddonKey) -> bool {
    addons.iter().any(|addon| {
        client
            .addon_key(addon)
            .is_ok_and(|installed| &installed == key)
            && addon.as_core().manifest.resources.iter().any(|resource| {
                let manifest = &addon.as_core().manifest;
                let (name, types) = match resource {
                    ManifestResource::Short(name) => (name, &manifest.types),
                    ManifestResource::Full { name, types, .. } => {
                        (name, types.as_ref().unwrap_or(&manifest.types))
                    }
                };
                name == "meta" && types.iter().any(|kind| kind == "movie")
            })
    })
}

async fn meta(
    state: &Arc<State>,
    client: &AddonClient,
    addons: &[Descriptor],
) -> Result<Option<AddonKey>, InstallError> {
    let saved: Option<AddonKey> = read(state, "metaAddon").await?;
    if saved
        .as_ref()
        .is_some_and(|key| !valid_meta(client, addons, key))
    {
        write::<AddonKey>(state, "metaAddon", None).await?;
        return Ok(None);
    }
    Ok(saved)
}

async fn home(
    state: &Arc<State>,
    client: &AddonClient,
    addons: &[Descriptor],
) -> Result<Option<CatalogRef>, InstallError> {
    let saved: Option<CatalogRef> = read(state, "homeCatalog").await?;
    let catalogs = client.catalogs(addons);
    let current = saved
        .as_ref()
        .and_then(|saved| {
            catalogs.iter().find(|catalog| {
                catalog.addon == saved.addon && catalog.catalog_id == saved.catalog_id
            })
        })
        .cloned();
    if let Some(current) = current {
        return Ok(Some(current));
    }
    if saved.is_some() {
        write::<CatalogRef>(state, "homeCatalog", None).await?;
    }
    Ok(catalogs.into_iter().next())
}

pub(super) async fn cleanup(
    state: &Arc<State>,
    client: &AddonClient,
    addons: &[Descriptor],
    removed: Option<AddonKey>,
) -> Result<(), InstallError> {
    if let Some(removed) = removed {
        state
            .blocking(move |store| store.clear_addon_catalogs(removed.as_str()))
            .await
            .map_err(|_| InstallError::Storage)?;
    }
    meta(state, client, addons).await?;
    home(state, client, addons).await?;
    Ok(())
}

impl CoreSession {
    /// Persists a movie metadata source; `None` selects Automatic. Requires sign-in.
    pub async fn set_meta_source(&mut self, source: Option<AddonKey>) -> Result<(), InstallError> {
        self.require_addon_account()?;
        self.settle_addons().await?;
        let client = self.addon_client().await?;
        if source
            .as_ref()
            .is_some_and(|key| !valid_meta(&client, &self.installed_addons(), key))
        {
            return Err(InstallError::InvalidSelection);
        }
        write(&self.state, "metaAddon", source).await
    }

    /// Reads the movie metadata preference, removing stale selections automatically.
    pub async fn meta_source(&mut self) -> Result<Option<AddonKey>, InstallError> {
        self.settle_addons().await?;
        let client = self.addon_client().await?;
        meta(&self.state, &client, &self.installed_addons()).await
    }

    /// Persists an available movie catalog; `None` follows the first current catalog.
    pub async fn set_home_catalog(
        &mut self,
        catalog: Option<CatalogRef>,
    ) -> Result<(), InstallError> {
        self.require_addon_account()?;
        self.settle_addons().await?;
        let client = self.addon_client().await?;
        let current = match catalog {
            Some(selected) => Some(
                client
                    .catalogs(&self.installed_addons())
                    .into_iter()
                    .find(|catalog| {
                        catalog.addon == selected.addon && catalog.catalog_id == selected.catalog_id
                    })
                    .ok_or(InstallError::InvalidSelection)?,
            ),
            None => None,
        };
        write(&self.state, "homeCatalog", current).await
    }

    /// Reads the Home selection, falling back to the first available movie catalog.
    pub async fn home_catalog(&mut self) -> Result<Option<CatalogRef>, InstallError> {
        self.settle_addons().await?;
        let client = self.addon_client().await?;
        let addons = if self.is_signed_in() {
            self.installed_addons()
        } else {
            vec![]
        };
        home(&self.state, &client, &addons).await
    }
}
