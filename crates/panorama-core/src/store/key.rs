use std::fmt;

use super::StoreError;

/// Maximum UTF-8 byte length of a general key component; movie IDs allow 800.
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

    /// Like [`Key::catalog`] for a catalog of the given addon type. Movie catalogs keep the
    /// untyped form so existing caches stay valid; other types get a segment of their own.
    pub fn typed_catalog(addon_id: &str, kind: &str, catalog_id: &str) -> Result<Self, StoreError> {
        let Self(key) = Self::catalog(addon_id, catalog_id)?;
        if kind == "movie" {
            return Ok(Self(key));
        }
        Ok(Self(format!("catalog:{addon_id}|{kind}|{catalog_id}")))
    }

    /// Identifies cached film details; supports 200 Unicode characters (800 bytes).
    pub fn meta(film_id: &str) -> Result<Self, StoreError> {
        if film_id.chars().count() <= 200 {
            validate_with_limit(film_id, 800, "film ID exceeds 800 bytes")?;
        } else {
            validate(film_id)?;
        }
        Ok(Self(format!("meta:{film_id}")))
    }

    /// Like [`Key::meta`] for a title of the given addon type: a film and a show can share an
    /// id. Movies keep the untyped form so existing caches stay valid.
    pub fn typed_meta(kind: &str, film_id: &str) -> Result<Self, StoreError> {
        let Self(key) = Self::meta(film_id)?;
        if kind == "movie" {
            return Ok(Self(key));
        }
        Ok(Self(format!("meta:{kind}|{film_id}")))
    }

    /// Identifies a film's cached awards; validated like [`Key::meta`]. It lives under `meta:`
    /// so the film-details cache policy (age, count and sign-out pruning) covers it without a
    /// schema change.
    pub fn awards(film_id: &str) -> Result<Self, StoreError> {
        let Self(meta) = Self::meta(film_id)?;
        Ok(Self(format!("meta:awards|{}", &meta["meta:".len()..])))
    }

    /// Identifies a film's cached original title, under `meta:` as [`Key::awards`] is.
    pub fn original_title(film_id: &str) -> Result<Self, StoreError> {
        let Self(meta) = Self::meta(film_id)?;
        Ok(Self(format!("meta:foreign|{}", &meta["meta:".len()..])))
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
    validate_with_limit(
        component,
        MAX_COMPONENT_BYTES,
        "component exceeds 512 bytes",
    )
}

fn validate_with_limit(
    component: &str,
    limit: usize,
    length_reason: &'static str,
) -> Result<(), StoreError> {
    let reason = if component.is_empty() {
        Some("components cannot be empty")
    } else if component.len() > limit {
        Some(length_reason)
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
