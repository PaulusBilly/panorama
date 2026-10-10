use std::fmt;

use super::StoreError;

/// Maximum UTF-8 byte length of an individual key component.
pub const MAX_COMPONENT_BYTES: usize = 512;

/// The persistence policy associated with a key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyClass {
    /// Saved Stremio core state, removed on sign-out.
    Core,
    /// Catalogs and film details, subject to pruning and sign-out.
    Cache,
    /// Preferences, retained across sign-out.
    Pref,
}

/// A validated, namespaced identifier. Pass addon IDs, never transport URLs.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Key(String);

impl Key {
    /// Identifies a saved core bucket, including its own `schema_version` bucket.
    pub fn core(bucket: &str) -> Result<Self, StoreError> {
        Self::single("core", bucket)
    }

    /// Identifies a catalog by addon ID and catalog ID; embedded `|` is rejected.
    pub fn catalog(addon_id: &str, catalog_id: &str) -> Result<Self, StoreError> {
        validate(addon_id)?;
        validate(catalog_id)?;
        if addon_id.contains('|') || catalog_id.contains('|') {
            return Err(StoreError::InvalidKey {
                reason: "catalog components cannot contain |",
            });
        }
        Ok(Self(format!("catalog:{addon_id}|{catalog_id}")))
    }

    /// Identifies cached film details by film ID.
    pub fn meta(film_id: &str) -> Result<Self, StoreError> {
        Self::single("meta", film_id)
    }

    /// Identifies a preference by name.
    pub fn pref(name: &str) -> Result<Self, StoreError> {
        Self::single("pref", name)
    }

    /// Returns the key's persistence policy.
    pub fn class(&self) -> KeyClass {
        if self.0.starts_with("core:") {
            KeyClass::Core
        } else if self.0.starts_with("pref:") {
            KeyClass::Pref
        } else {
            KeyClass::Cache
        }
    }

    /// Returns the validated storage key.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn single(namespace: &str, component: &str) -> Result<Self, StoreError> {
        validate(component)?;
        Ok(Self(format!("{namespace}:{component}")))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn validate(component: &str) -> Result<(), StoreError> {
    let reason = if component.is_empty() {
        Some("components cannot be empty")
    } else if component.len() > MAX_COMPONENT_BYTES {
        Some("component exceeds 512 bytes")
    } else if component.chars().any(char::is_control) {
        Some("components cannot contain control characters")
    } else if component.contains("://") || component.starts_with("//") {
        Some("components must be identifiers, not URLs")
    } else {
        None
    };
    match reason {
        Some(reason) => Err(StoreError::InvalidKey { reason }),
        None => Ok(()),
    }
}
