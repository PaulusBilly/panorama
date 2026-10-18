use std::{fmt, sync::Arc};

use super::CoreErrorKind;

/// A cloned account profile whose debug representation redacts all account data.
#[derive(Clone, PartialEq, Eq)]
pub struct Profile(pub(super) stremio_core::types::profile::Profile);

impl Profile {
    /// Borrows the core profile for explicit account-field access.
    pub fn as_core(&self) -> &stremio_core::types::profile::Profile {
        &self.0
    }

    /// Returns whether this profile holds an authenticated account.
    pub fn is_signed_in(&self) -> bool {
        self.0.auth.is_some()
    }

    /// Returns installed addons in account order, with redacted debug output.
    pub fn installed_addons(&self) -> Vec<Descriptor> {
        self.0
            .addons
            .iter()
            .cloned()
            .map(Descriptor::from_core)
            .collect()
    }
}

impl fmt::Debug for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Profile")
            .field("signed_in", &self.is_signed_in())
            .field("addon_count", &self.0.addons.len())
            .finish_non_exhaustive()
    }
}

/// A cloned addon descriptor that never formats its transport URL or manifest.
#[derive(Clone, PartialEq, Eq)]
pub struct Descriptor(pub(super) Arc<stremio_core::types::addon::Descriptor>);

impl Descriptor {
    /// Wraps an explicitly supplied core descriptor with redacted debug formatting.
    pub fn from_core(descriptor: stremio_core::types::addon::Descriptor) -> Self {
        Self(Arc::new(descriptor))
    }

    /// Borrows the core descriptor for explicit manifest or transport access.
    pub fn as_core(&self) -> &stremio_core::types::addon::Descriptor {
        &self.0
    }

    pub(crate) fn shared_core(&self) -> Arc<stremio_core::types::addon::Descriptor> {
        Arc::clone(&self.0)
    }
}

impl fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Descriptor { .. }")
    }
}

/// Untrusted display fields of an installed addon; debug formatting redacts all of them.
/// Callers must cap lengths and render the text as plain text.
pub struct AddonSummary {
    /// Manifest name, unbounded.
    pub name: String,
    /// Semver version.
    pub version: String,
    /// HTTPS logo, if supplied.
    pub logo: Option<url::Url>,
    /// Resource names such as `catalog`, `meta` and `stream`.
    pub resources: Vec<String>,
    /// Supported media types.
    pub types: Vec<String>,
    /// Built-in addon that cannot be removed.
    pub protected: bool,
    /// Offers a configuration page over HTTPS, as `CoreSession::configure_url` requires.
    pub configurable: bool,
    /// Offers movie metadata, which makes it eligible as the Film details source.
    pub movie_meta: bool,
}

impl fmt::Debug for AddonSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AddonSummary { .. }")
    }
}

impl Descriptor {
    /// Summarizes the manifest for display without exposing the transport URL.
    pub fn summary(&self) -> AddonSummary {
        use stremio_core::types::addon::ManifestResource;
        let core = self.as_core();
        let manifest = &core.manifest;
        let mut movie_meta = false;
        let resources = manifest
            .resources
            .iter()
            .map(|resource| {
                let (name, types) = match resource {
                    ManifestResource::Short(name) => (name, &manifest.types),
                    ManifestResource::Full { name, types, .. } => {
                        (name, types.as_ref().unwrap_or(&manifest.types))
                    }
                };
                movie_meta |= name == "meta" && types.iter().any(|kind| kind == "movie");
                name.clone()
            })
            .collect();
        AddonSummary {
            name: manifest.name.clone(),
            version: manifest.version.to_string(),
            logo: manifest.logo.clone(),
            resources,
            types: manifest.types.clone(),
            protected: core.flags.protected,
            configurable: manifest.behavior_hints.configurable
                && core.transport_url.scheme() == "https",
            movie_meta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(manifest: serde_json::Value, url: &str) -> Descriptor {
        Descriptor::from_core(
            serde_json::from_value(serde_json::json!({
                "transportUrl": url,
                "manifest": manifest,
                "flags": { "protected": true },
            }))
            .expect("fixture descriptor"),
        )
    }

    #[test]
    fn summary_reports_display_fields_and_movie_meta() {
        let summary = descriptor(
            serde_json::json!({
                "id": "a", "name": "A", "version": "1.2.3", "types": ["series"],
                "resources": ["catalog", {"name": "meta", "types": ["movie"]}],
                "behaviorHints": {"configurable": true},
            }),
            "https://example.test/manifest.json",
        )
        .summary();
        assert_eq!(summary.version, "1.2.3");
        assert_eq!(summary.resources, ["catalog", "meta"]);
        assert!(summary.protected && summary.configurable && summary.movie_meta);
    }

    #[test]
    fn summary_requires_https_and_movie_type_for_those_flags() {
        let summary = descriptor(
            serde_json::json!({
                "id": "a", "name": "A", "version": "1.0.0", "types": ["series"],
                "resources": ["meta"], "behaviorHints": {"configurable": true},
            }),
            "http://example.test/manifest.json",
        )
        .summary();
        assert!(!summary.configurable && !summary.movie_meta);
        assert_eq!(format!("{summary:?}"), "AddonSummary { .. }");
    }
}

/// Sanitized notifications for UI subscribers; no raw core events are forwarded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreChange {
    /// Account or settings changed.
    ProfileChanged,
    /// Installed addons or their account order changed.
    AddonsChanged,
    /// Library contents changed.
    LibraryChanged,
    /// An operation failed with a sanitized category.
    Error(CoreErrorKind),
}
