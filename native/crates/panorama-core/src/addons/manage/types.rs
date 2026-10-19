use std::fmt;

use stremio_core::types::addon::Manifest;
use url::Url;

use crate::addons::AddonKey;

/// Sanitized management failures, without upstream diagnostics or payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallError {
    /// URL violates installation policy.
    InvalidUrl,
    /// Manifest JSON or required fields are invalid.
    InvalidManifest,
    /// Manifest response exceeds 256 KiB.
    TooLarge,
    /// Network or redirect policy failed.
    Network,
    /// Operation exceeded its deadline.
    Timeout,
    /// Addon management requires authentication.
    SignedOut,
    /// Addon cannot be removed or upgraded.
    Protected,
    /// Different transport claims a protected or official identity.
    ImpersonatesProtected,
    /// Order is not a permutation of installed addons.
    InvalidOrder,
    /// Installation or preference selection is unavailable.
    InvalidSelection,
    /// Confirmation token is expired, forged, consumed or from another session.
    InvalidToken,
    /// Configuration must be completed before installation.
    ConfigurationRequired,
    /// Core collection is locked or action was rejected.
    Core,
    /// Collection API push failed; core retains its local change.
    ApiPush,
    /// Persistent storage failed.
    Storage,
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for InstallError {}

/// Opaque, single-use, session-bound confirmation for an immutable manifest.
pub struct PreviewToken(pub(crate) [u8; 32]);

impl fmt::Debug for PreviewToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PreviewToken { .. }")
    }
}

/// Display summary of a validated manifest; debug formatting redacts all fields.
pub struct AddonPreview {
    /// Manifest identifier.
    pub id: String,
    /// Semver version.
    pub version: String,
    /// Trimmed display name.
    pub name: String,
    /// Description, truncated to 2,000 characters.
    pub description: Option<String>,
    /// HTTPS logo, if supplied.
    pub logo: Option<Url>,
    /// HTTPS background, if supplied.
    pub background: Option<Url>,
    /// Supported resource names.
    pub resources: Vec<String>,
    /// Supported media types.
    pub types: Vec<String>,
    /// Catalog display names, falling back to their identifiers.
    pub catalog_names: Vec<String>,
    /// Addon offers a configuration page.
    pub configurable: bool,
    /// Addon requires configuration before installation.
    pub configuration_required: bool,
    /// This transport is already installed.
    pub already_installed: bool,
    /// Installation replaced by this manifest identity.
    pub replaces: Option<AddonKey>,
    /// Pass this token to `CoreSession::install` after confirmation.
    pub token: PreviewToken,
}

impl AddonPreview {
    pub(crate) fn new(
        manifest: &Manifest,
        already_installed: bool,
        replaces: Option<AddonKey>,
        token: PreviewToken,
    ) -> Self {
        Self {
            id: manifest.id.clone(),
            version: manifest.version.to_string(),
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            logo: manifest.logo.clone(),
            background: manifest.background.clone(),
            resources: manifest
                .resources
                .iter()
                .map(|resource| match resource {
                    stremio_core::types::addon::ManifestResource::Short(name)
                    | stremio_core::types::addon::ManifestResource::Full { name, .. } => {
                        name.clone()
                    }
                })
                .collect(),
            types: manifest.types.clone(),
            catalog_names: manifest
                .catalogs
                .iter()
                .map(|catalog| catalog.name.as_ref().unwrap_or(&catalog.id).clone())
                .collect(),
            configurable: manifest.behavior_hints.configurable,
            configuration_required: manifest.behavior_hints.configuration_required,
            already_installed,
            replaces,
            token,
        }
    }
}

impl fmt::Debug for AddonPreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AddonPreview { .. }")
    }
}
