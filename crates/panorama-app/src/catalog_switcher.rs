//! State of Home's catalog switcher: menu presence, keyboard cursor and the grid swap.
use crate::{addons_screen::model::CatalogGroup, theme::motion::*, transition::Tween};
use gpui::FocusHandle;
use panorama_core::addons::CatalogRef;
use std::time::{Duration, Instant};

/// Height of the switcher row: 48 above, 44 of content, 24 below.
pub const BLOCK_HEIGHT: f32 = 116.0;

/// Whether an account has enough eligible catalogs to offer a choice.
pub fn offers_choice(groups: &[CatalogGroup]) -> bool {
    groups
        .iter()
        .map(|group| group.catalogs.len())
        .sum::<usize>()
        > 1
}

/// Catalogs of every group in menu order.
pub fn flatten(groups: &[CatalogGroup]) -> Vec<CatalogRef> {
    groups
        .iter()
        .flat_map(|group| group.catalogs.iter().cloned())
        .collect()
}

fn same(a: &CatalogRef, b: &CatalogRef) -> bool {
    a.addon == b.addon && a.kind == b.kind && a.catalog_id == b.catalog_id
}

/// The entry of `flat` that names `wanted`, if the catalog is still offered.
pub fn offered(flat: &[CatalogRef], wanted: Option<&CatalogRef>) -> Option<CatalogRef> {
    flat.iter()
        .find(|c| wanted.is_some_and(|w| same(c, w)))
        .cloned()
}

/// A remembered catalog survives only while an installed addon still offers it.
pub fn reconcile(remembered: Option<CatalogRef>, flat: &[CatalogRef]) -> Option<CatalogRef> {
    offered(flat, remembered.as_ref())
}

/// The catalog the trigger names: the pending pick, the remembered one, the one on screen,
/// then the first. Entries no longer offered are ignored, as the core does.
pub fn current(
    flat: &[CatalogRef],
    pending: Option<&CatalogRef>,
    remembered: Option<&CatalogRef>,
    shown: Option<&CatalogRef>,
) -> Option<CatalogRef> {
    [pending, remembered, shown]
        .into_iter()
        .find_map(|wanted| offered(flat, wanted))
        .or_else(|| flat.first().cloned())
}

/// Move a keyboard cursor, stopping at both ends.
pub fn step(len: usize, active: usize, delta: isize) -> usize {
    active
        .saturating_add_signed(delta)
        .min(len.saturating_sub(1))
}

/// Menu, chevron and grid-swap timelines with their focus targets.
pub struct CatalogSwitcher {
    /// Whether the menu accepts input.
    pub open: bool,
    /// Chevron rotation and menu presence share one 0..1 timeline.
    pub presence: Tween,
    /// Trigger focus.
    pub trigger: FocusHandle,
    /// Menu focus, which owns arrow keys while open.
    pub menu: FocusHandle,
    /// Keyboard cursor over the flattened catalogs.
    pub active: usize,
    closed: Option<Instant>,
    fade: Tween,
    /// A chosen catalog waiting for the cards to finish fading out.
    pub pending: Option<CatalogRef>,
    /// The new catalog's first page has not arrived yet.
    pub awaiting: bool,
}

impl CatalogSwitcher {
    /// Allocate focus targets for one Home entry.
    pub fn new(trigger: FocusHandle, menu: FocusHandle) -> Self {
        Self {
            open: false,
            presence: Tween::fixed(0.0),
            trigger,
            menu,
            active: 0,
            closed: None,
            fade: Tween::fixed(1.0),
            pending: None,
            awaiting: false,
        }
    }

    /// Open or close with the account menu's motion.
    pub fn set_open(&mut self, open: bool) {
        if self.open == open {
            return;
        }
        self.open = open;
        if !open {
            self.closed = Some(Instant::now());
        }
        self.presence.retarget(
            if open { 1.0 } else { 0.0 },
            if open { MENU_ENTER } else { MENU_EXIT },
            Some(if open { EASE_MENU } else { EASE_IN }),
            Instant::now(),
        );
    }

    /// Trigger click. A press that just dismissed the menu from outside must not reopen it.
    pub fn toggle(&mut self) {
        let recent = self
            .closed
            .is_some_and(|at| at.elapsed() < Duration::from_millis(150));
        if self.open || !recent {
            self.set_open(!self.open);
        }
    }

    /// Whether the menu is still painted.
    pub fn visible(&self, reduced: bool) -> bool {
        self.open || self.presence.moving(Instant::now(), reduced)
    }

    /// Whether any switcher timeline needs frames.
    pub fn moving(&self, now: Instant, reduced: bool) -> bool {
        self.presence.moving(now, reduced) || self.fade.moving(now, reduced)
    }

    /// Close the menu and begin fading the current cards out.
    pub fn choose(&mut self, catalog: CatalogRef) {
        self.set_open(false);
        self.pending = Some(catalog);
        self.fade
            .retarget(0.0, DURATION_FAST, Some(EASE_DEFAULT), Instant::now());
    }

    /// The chosen catalog, once the fade-out has finished.
    pub fn take_ready(&mut self, now: Instant, reduced: bool) -> Option<CatalogRef> {
        if self.pending.is_some() && !self.fade.moving(now, reduced) {
            self.awaiting = true;
            return self.pending.take();
        }
        None
    }

    /// Fade the new cards in, or skip the fade when nothing arrived.
    pub fn arrived(&mut self, cards: bool) {
        self.awaiting = false;
        if cards {
            self.fade
                .retarget(1.0, DURATION_FAST, Some(EASE_DEFAULT), Instant::now());
        } else {
            self.fade = Tween::fixed(1.0);
        }
    }

    /// Grid opacity; skeletons stay fully visible while the next page loads.
    pub fn grid_opacity(&self, empty: bool, now: Instant, reduced: bool) -> f32 {
        if self.awaiting && empty {
            1.0
        } else {
            self.fade.value(now, reduced)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use panorama_core::addons::{AddonKey, Kind};

    fn group(name: &str, count: usize) -> CatalogGroup {
        let addon: AddonKey = serde_json::from_value(format!("{name}#1").into()).expect("key");
        CatalogGroup {
            addon: name.into(),
            catalogs: (0..count)
                .map(|i| CatalogRef {
                    addon: addon.clone(),
                    catalog_id: i.to_string(),
                    name: i.to_string(),
                    kind: Kind::Movie,
                })
                .collect(),
        }
    }

    #[test]
    fn hidden_with_one_catalog() {
        assert!(!offers_choice(&[]));
        assert!(!offers_choice(&[group("a", 1)]));
        assert!(offers_choice(&[group("a", 1), group("b", 1)]));
        assert!(offers_choice(&[group("a", 2)]));
    }

    #[test]
    fn flatten_keeps_group_order() {
        let flat = flatten(&[group("a", 2), group("b", 1)]);
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[2].addon.as_str(), "b#1");
    }

    #[test]
    fn removed_remembered_catalog_is_dropped_and_never_names_the_trigger() {
        let flat = flatten(&[group("a", 2), group("b", 1)]);
        let gone = group("gone", 1).catalogs[0].clone();
        assert_eq!(reconcile(Some(gone.clone()), &flat), None);
        assert_eq!(
            reconcile(Some(flat[1].clone()), &flat),
            Some(flat[1].clone())
        );
        assert_eq!(
            current(&flat, None, Some(&gone), None),
            Some(flat[0].clone())
        );
        assert_eq!(
            current(&flat, Some(&gone), Some(&flat[2]), None),
            Some(flat[2].clone())
        );
        assert_eq!(
            current(&flat, Some(&flat[1]), Some(&flat[2]), None),
            Some(flat[1].clone())
        );
        assert_eq!(
            current(&flat, None, None, Some(&flat[2])),
            Some(flat[2].clone())
        );
        assert_eq!(current(&[], Some(&gone), None, None), None);
    }

    #[test]
    fn keyboard_cursor_stops_at_the_ends() {
        assert_eq!(step(3, 0, -1), 0);
        assert_eq!(step(3, 2, 1), 2);
        assert_eq!(step(3, 1, 1), 2);
        assert_eq!(step(0, 0, 1), 0);
    }
}
