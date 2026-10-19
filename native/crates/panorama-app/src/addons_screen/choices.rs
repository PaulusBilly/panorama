//! Pure Addons helpers: film details choices, Home catalog groups, screen gating.
use super::model::{NAME_CAP, display_text};
use panorama_core::addons::{AddonKey, CatalogRef};

/// An installed addon that can supply film details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetaChoice {
    /// Installation identity.
    pub key: AddonKey,
    /// Capped plain-text name.
    pub name: String,
}

/// The saved source, or Automatic when it is gone from the choices.
pub fn meta_selection(saved: Option<AddonKey>, choices: &[MetaChoice]) -> Option<AddonKey> {
    saved.filter(|key| choices.iter().any(|choice| &choice.key == key))
}

/// Next radio index for an arrow key, wrapping at both ends.
pub fn radio_step(len: usize, current: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as isize + delta).rem_euclid(len as isize) as usize
}

/// Catalogs of one addon in the Home switcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogGroup {
    /// Addon display name.
    pub addon: String,
    /// Catalogs in the addon's own order.
    pub catalogs: Vec<CatalogRef>,
}

/// Group catalogs by addon in account order. Catalogs from an addon missing from
/// `addons` (the Cinemeta fallback of a signed-out account) land in a "Cinemeta" group.
pub fn catalog_groups(addons: &[(AddonKey, String)], catalogs: &[CatalogRef]) -> Vec<CatalogGroup> {
    let mut groups: Vec<(AddonKey, CatalogGroup)> = vec![];
    for catalog in catalogs {
        if let Some((_, group)) = groups.iter_mut().find(|(key, _)| key == &catalog.addon) {
            group.catalogs.push(catalog.clone());
            continue;
        }
        let addon = addons
            .iter()
            .find(|(key, _)| key == &catalog.addon)
            .map_or_else(
                || "Cinemeta".into(),
                |(_, name)| display_text(name, NAME_CAP),
            );
        groups.push((
            catalog.addon.clone(),
            CatalogGroup {
                addon,
                catalogs: vec![catalog.clone()],
            },
        ));
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

/// Whether the screen shows the sections, a sign-in prompt or a load state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    /// Account is not signed in.
    SignedOut,
    /// First list load in flight.
    Loading,
    /// The list could not be loaded.
    Failed,
    /// Sections are shown.
    Ready,
}

/// Gate the three sections behind a signed-in account and a loaded list.
pub fn screen(signed_in: bool, loading: bool, failed: bool) -> Screen {
    if !signed_in {
        Screen::SignedOut
    } else if failed {
        Screen::Failed
    } else if loading {
        Screen::Loading
    } else {
        Screen::Ready
    }
}
