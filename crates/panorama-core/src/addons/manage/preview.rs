use stremio_core::types::addon::Manifest;

use super::InstallError;
use crate::{
    addons::{AddonClient, AddonKey},
    stremio::Descriptor,
};

pub(crate) fn parse_manifest(bytes: &[u8]) -> Result<Manifest, InstallError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| InstallError::InvalidManifest)?;
    if value
        .get("catalogs")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|catalogs| catalogs.len() > 200)
    {
        return Err(InstallError::InvalidManifest);
    }
    let mut manifest: Manifest =
        serde_json::from_value(value).map_err(|_| InstallError::InvalidManifest)?;
    if manifest.id.is_empty()
        || manifest.id.chars().count() > 200
        || manifest
            .id
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
        || manifest.name.trim().chars().count() > 200
        || manifest.resources.is_empty()
        || manifest.types.is_empty()
    {
        return Err(InstallError::InvalidManifest);
    }
    manifest.name = manifest.name.trim().to_owned();
    manifest.description = manifest
        .description
        .map(|text| text.chars().take(2000).collect());
    manifest.logo = manifest.logo.filter(|url| url.scheme() == "https");
    manifest.background = manifest.background.filter(|url| url.scheme() == "https");
    Ok(manifest)
}

pub(crate) fn replacement(
    addons: &[Descriptor],
    descriptor: &Descriptor,
    client: &AddonClient,
) -> Result<Option<AddonKey>, InstallError> {
    let candidate = descriptor.as_core();
    if addons.iter().any(|installed| {
        let addon = installed.as_core();
        addon.manifest.id == candidate.manifest.id
            && addon.transport_url != candidate.transport_url
            && (addon.flags.protected || addon.flags.official)
    }) {
        return Err(InstallError::ImpersonatesProtected);
    }
    let installed = if let Some(installed) = addons
        .iter()
        .find(|addon| addon.as_core().transport_url == candidate.transport_url)
    {
        Some(installed)
    } else {
        let mut matches = addons
            .iter()
            .filter(|addon| addon.as_core().manifest.id == candidate.manifest.id);
        let first = matches.next();
        if matches.next().is_none() {
            first
        } else {
            None
        }
    };
    installed
        .map(|addon| {
            client
                .addon_key(addon)
                .map_err(|_| InstallError::InvalidManifest)
        })
        .transpose()
}
