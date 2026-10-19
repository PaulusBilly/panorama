//! Transitions behind the Addons actions, free of GPUI so tests can drive them.
use super::{
    choices::{MetaChoice, meta_selection},
    model::{AddonError, InstalledRow, RowPending},
};
use panorama_core::addons::AddonKey;
use std::collections::HashSet;

/// Numbers requests so that only the latest one's reply may be applied.
#[derive(Debug, Default)]
pub struct Latest(u64);

impl Latest {
    /// Start a request, which also makes every earlier one stale.
    pub fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }

    /// Whether `id` is still the newest request.
    pub fn is_current(&self, id: u64) -> bool {
        self.0 == id
    }
}

/// Keys a reorder may send: every row that is not on its way out.
pub fn visible_order(rows: &[InstalledRow], leaving: &HashSet<AddonKey>) -> Vec<AddonKey> {
    rows.iter()
        .filter(|row| !leaving.contains(&row.key))
        .map(|row| row.key.clone())
        .collect()
}

/// Sort rows by `order`, keeping rows it does not name (leaving ones) last. Returns each
/// moved key with how many places it rose (positive) or fell (negative).
pub fn reorder(rows: &mut [InstalledRow], order: &[AddonKey]) -> Vec<(AddonKey, isize)> {
    let before: Vec<_> = rows.iter().map(|row| row.key.clone()).collect();
    rows.sort_by_key(|row| {
        order
            .iter()
            .position(|key| key == &row.key)
            .unwrap_or(usize::MAX)
    });
    rows.iter()
        .enumerate()
        .filter_map(|(new, row)| {
            let old = before.iter().position(|key| key == &row.key)?;
            (old != new).then(|| (row.key.clone(), old as isize - new as isize))
        })
        .collect()
}

/// Fold a fresh list into the shown one: retained rows keep their pending state, rows that
/// are leaving stay (last) until their fade ends even if the core no longer lists them.
/// Also returns the keys that were not shown before.
pub fn merge_rows(
    current: &[InstalledRow],
    leaving: &HashSet<AddonKey>,
    mut fresh: Vec<InstalledRow>,
) -> (Vec<InstalledRow>, Vec<AddonKey>) {
    let mut added = vec![];
    for row in &mut fresh {
        match current.iter().find(|old| old.key == row.key) {
            Some(old) => row.pending = old.pending,
            None => added.push(row.key.clone()),
        }
    }
    fresh.retain(|row| !leaving.contains(&row.key));
    fresh.extend(
        current
            .iter()
            .filter(|row| leaving.contains(&row.key))
            .cloned(),
    );
    (fresh, added)
}

/// Drop rows whose fade has ended.
pub fn finish_leaving(
    rows: &mut Vec<InstalledRow>,
    leaving: &mut HashSet<AddonKey>,
    gone: &[AddonKey],
) {
    for key in gone {
        leaving.remove(key);
        rows.retain(|row| &row.key != key);
    }
}

/// Where focus goes once `removed` disappears: the Remove button of the next row, or of the
/// previous one when it was last. Rows without a Remove button (built in, or mid-action)
/// are skipped in the same direction before trying the other side.
pub fn focus_after_remove(
    rows: &[InstalledRow],
    leaving: &HashSet<AddonKey>,
    removed: &AddonKey,
) -> Option<AddonKey> {
    let visible: Vec<_> = rows
        .iter()
        .filter(|row| !leaving.contains(&row.key))
        .collect();
    let at = visible.iter().position(|row| &row.key == removed)?;
    let can = |row: &&InstalledRow| {
        !row.protected && matches!(row.pending, RowPending::None | RowPending::Error(_))
    };
    visible[at + 1..]
        .iter()
        .chain(visible[..at].iter().rev())
        .find(|row| can(row))
        .map(|row| row.key.clone())
}

/// The Film details choice as the screen holds it, resynced from the core after every reply.
#[derive(Debug, Default)]
pub struct MetaState {
    /// Addons that can supply film details.
    pub choices: Vec<MetaChoice>,
    /// Radio shown as selected; `None` is Automatic.
    pub selected: Option<AddonKey>,
    /// Why the last save failed.
    pub error: Option<AddonError>,
    /// What the core last reported as saved, before the choices filter.
    saved: Option<AddonKey>,
    latest: Latest,
    pending: bool,
}

impl MetaState {
    /// Adopt what the core holds. While a save is in flight only the choices change, as its
    /// reply carries the authoritative selection.
    pub fn take(&mut self, choices: Vec<MetaChoice>, saved: Option<AddonKey>) {
        if !self.pending {
            self.selected = meta_selection(saved.clone(), &choices);
            self.saved = saved;
        }
        self.choices = choices;
    }

    /// Select `source` and return the request id to save it under, or `None` when it is
    /// already both shown and saved.
    pub fn choose(&mut self, source: Option<AddonKey>) -> Option<u64> {
        if source == self.selected && source == self.saved {
            return None;
        }
        self.selected = source;
        self.error = None;
        self.pending = true;
        Some(self.latest.next())
    }

    /// Apply a save's reply unless a newer save superseded it. The snapshot, when readable,
    /// decides the selection; otherwise a failure falls back to the last saved source.
    pub fn reply(
        &mut self,
        id: u64,
        result: Result<(), AddonError>,
        snapshot: Option<(Vec<MetaChoice>, Option<AddonKey>)>,
    ) -> bool {
        if !self.latest.is_current(id) {
            return false;
        }
        self.pending = false;
        match snapshot {
            Some((choices, saved)) => self.take(choices, saved),
            None => match &result {
                Ok(()) => self.saved = self.selected.clone(),
                Err(_) => self.selected = meta_selection(self.saved.clone(), &self.choices),
            },
        }
        self.error = result.err();
        true
    }
}
