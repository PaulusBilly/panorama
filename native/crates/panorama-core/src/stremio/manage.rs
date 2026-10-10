use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use sha2::{Digest, Sha256};
use stremio_core::{runtime::msg::ActionCtx, types::addon::Descriptor as CoreDescriptor};
use tokio::{task::JoinHandle, time::Instant};
use tracing::{instrument::WithSubscriber, subscriber::NoSubscriber};
use url::Url;

use super::{CoreSession, Descriptor, fetch, session::Progress};
use crate::addons::{
    AddonClient, AddonKey, CoreAddonTransport,
    manage::{AddonPreview, AddonUrl, InstallError, PreviewToken, parse_manifest, replacement},
};

pub(super) struct Confirmation {
    pub(super) descriptor: Descriptor,
    pub(super) expires: Instant,
    replaces: Option<AddonKey>,
    binding: [u8; 32],
}

#[derive(Default)]
pub(super) struct Management {
    pub(super) tokens: HashMap<[u8; 32], Confirmation>,
    client: Option<AddonClient>,
    pub(super) pending: Option<JoinHandle<Result<(), InstallError>>>,
}

fn binding(descriptor: &Descriptor, replaces: &Option<AddonKey>) -> Result<[u8; 32], InstallError> {
    let bytes = serde_json::to_vec(&(descriptor.as_core(), replaces))
        .map_err(|_| InstallError::InvalidManifest)?;
    Ok(Sha256::digest(bytes).into())
}

impl CoreSession {
    pub(super) async fn addon_client(&mut self) -> Result<AddonClient, InstallError> {
        if let Some(client) = &self.management.client {
            return Ok(client.clone());
        }
        let store = Arc::clone(&self.state.store);
        let client = self
            .state
            .runtime
            .spawn(async move { AddonClient::new(store, CoreAddonTransport).await })
            .await
            .map_err(|_| InstallError::Storage)?
            .map_err(|_| InstallError::Storage)?;
        self.management.client = Some(client.clone());
        Ok(client)
    }

    pub(super) fn require_addon_account(&self) -> Result<(), InstallError> {
        if !self.is_signed_in() {
            return Err(InstallError::SignedOut);
        }
        if self.profile().as_core().addons_locked {
            return Err(InstallError::Core);
        }
        Ok(())
    }

    /// Fetches a bounded manifest and returns a ten-minute immutable confirmation.
    /// Requires sign-in; URLs, display fields and tokens have redacted Debug output.
    pub async fn preview(&mut self, url: AddonUrl) -> Result<AddonPreview, InstallError> {
        self.require_addon_account()?;
        self.settle_addons().await?;
        let state = Arc::clone(&self.state);
        let raw = url.as_str().to_owned();
        let bytes = self
            .state
            .runtime
            .spawn(
                async move { fetch::manifest(state, &raw).await }
                    .with_subscriber(NoSubscriber::default()),
            )
            .await
            .map_err(|_| InstallError::Network)??;
        let manifest = parse_manifest(&bytes)?;
        let descriptor = Descriptor::from_core(CoreDescriptor {
            transport_url: url.parsed,
            manifest,
            flags: Default::default(),
        });
        let client = self.addon_client().await?;
        client
            .addon_key(&descriptor)
            .map_err(|_| InstallError::InvalidManifest)?;
        let installed = self.installed_addons();
        let replaces = replacement(&installed, &descriptor, &client)?;
        let already_installed = installed
            .iter()
            .any(|addon| addon.as_core().transport_url == descriptor.as_core().transport_url);
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|_| InstallError::Core)?;
        let now = Instant::now();
        self.management
            .tokens
            .retain(|_, token| token.expires > now);
        let summary = AddonPreview::new(
            &descriptor.as_core().manifest,
            already_installed,
            replaces.clone(),
            PreviewToken(nonce),
        );
        self.management.tokens.insert(
            nonce,
            Confirmation {
                binding: binding(&descriptor, &replaces)?,
                replaces,
                descriptor,
                expires: now + Duration::from_secs(600),
            },
        );
        Ok(summary)
    }

    /// Installs exactly the previewed manifest, without another addon fetch.
    /// Core keeps local changes on API failure; returns `ApiPush` in that case.
    pub async fn install(&mut self, token: PreviewToken) -> Result<Vec<Descriptor>, InstallError> {
        self.require_addon_account()?;
        self.settle_addons().await?;
        let confirmation = self
            .management
            .tokens
            .remove(&token.0)
            .ok_or(InstallError::InvalidToken)?;
        if confirmation.expires <= Instant::now()
            || confirmation.binding != binding(&confirmation.descriptor, &confirmation.replaces)?
        {
            return Err(InstallError::InvalidToken);
        }
        let client = self.addon_client().await?;
        let installed = self.installed_addons();
        let mut descriptor = confirmation.descriptor;
        let replaced = confirmation.replaces;
        if replacement(&installed, &descriptor, &client)? != replaced {
            return Err(InstallError::InvalidToken);
        }
        let candidate = descriptor.as_core();
        if candidate.manifest.behavior_hints.configuration_required {
            return Err(InstallError::ConfigurationRequired);
        }
        let previous = match &replaced {
            Some(key) => Some(
                installed
                    .iter()
                    .find(|addon| {
                        client
                            .addon_key(addon)
                            .is_ok_and(|installed| &installed == key)
                    })
                    .ok_or(InstallError::InvalidToken)?,
            ),
            None => None,
        };
        if let Some(previous) = previous {
            if previous.as_core().transport_url == candidate.transport_url
                && previous.as_core().manifest == candidate.manifest
            {
                self.change_addons(ActionCtx::PushAddonsToAPI, None).await?;
                return Ok(self.installed_addons());
            }
            if previous.as_core().flags.protected {
                return Err(InstallError::Protected);
            }
            if previous.as_core().transport_url == candidate.transport_url {
                let mut core = candidate.clone();
                core.flags = previous.as_core().flags.clone();
                descriptor = Descriptor::from_core(core);
            }
        }
        if let Some(previous) = previous {
            let running = self.running.as_ref().ok_or(InstallError::Core)?;
            *running
                .replacement
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = Some(previous.as_core().clone());
        }
        self.change_addons(
            ActionCtx::InstallAddon(descriptor.as_core().clone()),
            replaced,
        )
        .await?;
        Ok(self.installed_addons())
    }

    /// Removes an unprotected installation and clears its cache and stale preferences.
    /// Cleanup also completes when core retains a local removal after API failure.
    pub async fn remove(&mut self, key: AddonKey) -> Result<Vec<Descriptor>, InstallError> {
        self.require_addon_account()?;
        self.settle_addons().await?;
        let client = self.addon_client().await?;
        let addon = self
            .installed_addons()
            .into_iter()
            .find(|addon| {
                client
                    .addon_key(addon)
                    .is_ok_and(|installed| installed == key)
            })
            .ok_or(InstallError::InvalidSelection)?;
        if addon.as_core().flags.protected {
            return Err(InstallError::Protected);
        }
        self.change_addons(
            ActionCtx::UninstallAddon(addon.as_core().clone()),
            Some(key),
        )
        .await?;
        Ok(self.installed_addons())
    }

    /// Persists a complete permutation; pinned core imposes no protected order rule.
    pub async fn reorder(&mut self, order: Vec<AddonKey>) -> Result<Vec<Descriptor>, InstallError> {
        self.require_addon_account()?;
        self.settle_addons().await?;
        let client = self.addon_client().await?;
        let installed = self.installed_addons();
        let keys = installed
            .iter()
            .map(|addon| {
                client
                    .addon_key(addon)
                    .map_err(|_| InstallError::InvalidOrder)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if order.len() != keys.len()
            || order.iter().collect::<HashSet<_>>() != keys.iter().collect::<HashSet<_>>()
            || order.iter().collect::<HashSet<_>>().len() != order.len()
        {
            return Err(InstallError::InvalidOrder);
        }
        let urls = order
            .iter()
            .map(|key| {
                keys.iter()
                    .position(|installed| installed == key)
                    .map(|index| installed[index].as_core().clone())
                    .ok_or(InstallError::InvalidOrder)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let running = self.running.as_ref().ok_or(InstallError::Core)?;
        *running.order.lock().unwrap_or_else(|p| p.into_inner()) = Some(urls);
        self.change_addons(ActionCtx::PushAddonsToAPI, None).await?;
        Ok(self.installed_addons())
    }

    /// Returns an HTTPS configuration page for an installed configurable addon.
    /// Async because installation identity salt is loaded off the caller's thread.
    pub async fn configure_url(&mut self, key: &AddonKey) -> Option<Url> {
        let client = self.addon_client().await.ok()?;
        let addon = self.installed_addons().into_iter().find(|addon| {
            client
                .addon_key(addon)
                .is_ok_and(|installed| &installed == key)
        })?;
        let descriptor = addon.as_core();
        if !descriptor.manifest.behavior_hints.configurable
            || descriptor.transport_url.scheme() != "https"
        {
            return None;
        }
        let mut url = descriptor.transport_url.clone();
        let path = url.path().strip_suffix("/manifest.json")?;
        let path = format!("{path}/configure");
        url.set_path(&path);
        url.set_query(None);
        url.set_fragment(None);
        if !url.username().is_empty() || url.password().is_some() {
            return None;
        }
        Some(url)
    }

    /// Returns the same salted identity used by the resource client and preferences.
    pub async fn addon_key(&mut self, addon: &Descriptor) -> Result<AddonKey, InstallError> {
        self.addon_client()
            .await?
            .addon_key(addon)
            .map_err(|_| InstallError::InvalidSelection)
    }

    pub(super) async fn settle_addons(&mut self) -> Result<(), InstallError> {
        let Some(pending) = self.management.pending.as_mut() else {
            return Ok(());
        };
        let result = pending.await.unwrap_or(Err(InstallError::Core));
        self.management.pending = None;
        result
    }

    async fn change_addons(
        &mut self,
        action: ActionCtx,
        removed: Option<AddonKey>,
    ) -> Result<(), InstallError> {
        let client = self.addon_client().await?;
        let running = self.running.as_ref().ok_or(InstallError::Core)?;
        let mut progress = running.progress.subscribe();
        let action_url = match &action {
            ActionCtx::InstallAddon(addon)
            | ActionCtx::UninstallAddon(addon)
            | ActionCtx::UpgradeAddon(addon) => Some(addon.transport_url.clone()),
            _ => None,
        };
        running.dispatch(action);
        let addons = self.installed_addons();
        let urls = addons
            .iter()
            .map(|addon| addon.as_core().transport_url.clone())
            .collect::<Vec<_>>();
        let state = Arc::clone(&self.state);
        self.management.pending = Some(self.state.runtime.spawn(async move {
            let cleanup = super::preferences::cleanup(&state, &client, &addons, removed).await;
            let pushed = tokio::time::timeout(fetch::TIMEOUT, async {
                loop {
                    match progress.recv().await {
                        Ok(Progress::Collection(received, result)) if received == urls => {
                            return result;
                        }
                        Ok(Progress::AddonRejected(url, error))
                            if action_url.as_ref() == Some(&url) =>
                        {
                            return Err(error);
                        }
                        Ok(_) => {}
                        Err(_) => return Err(InstallError::Core),
                    }
                }
            })
            .await
            .unwrap_or(Err(InstallError::Timeout));
            state.drain().await.map_err(|_| InstallError::Storage)?;
            cleanup?;
            pushed
        }));
        self.settle_addons().await
    }
}
