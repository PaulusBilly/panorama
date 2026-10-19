use std::fmt;

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
        self.0.addons.iter().cloned().map(Descriptor).collect()
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
pub struct Descriptor(pub(super) stremio_core::types::addon::Descriptor);

impl Descriptor {
    /// Borrows the core descriptor for explicit manifest or transport access.
    pub fn as_core(&self) -> &stremio_core::types::addon::Descriptor {
        &self.0
    }
}

impl fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Descriptor { .. }")
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
