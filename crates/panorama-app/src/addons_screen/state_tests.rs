use super::{
    choices::MetaChoice,
    model::{AddonError, InstalledRow, RowPending, move_row},
    state::*,
};
use panorama_core::addons::{AddonKey, manage::InstallError};
use std::collections::HashSet;

fn key(name: &str) -> AddonKey {
    serde_json::from_value(format!("{name}#1").into()).expect("addon key")
}

fn row(name: &str, protected: bool) -> InstalledRow {
    InstalledRow {
        key: key(name),
        name: name.into(),
        version: "1".into(),
        logo: None,
        capabilities: vec![],
        protected,
        configurable: false,
        pending: RowPending::None,
    }
}

fn rows(names: &[&str]) -> Vec<InstalledRow> {
    names.iter().map(|name| row(name, false)).collect()
}

fn names(rows: &[InstalledRow]) -> Vec<&str> {
    rows.iter().map(|row| row.name.as_str()).collect()
}

fn choice(name: &str) -> MetaChoice {
    MetaChoice {
        key: key(name),
        name: name.into(),
    }
}

#[test]
fn a_failed_move_reverts_to_the_order_that_was_sent_from() {
    let mut list = rows(&["a", "b", "c"]);
    let leaving = HashSet::new();
    let before = visible_order(&list, &leaving);
    let next = move_row(&before, &key("c"), -1).expect("move");
    let moved = reorder(&mut list, &next);
    assert_eq!(names(&list), ["a", "c", "b"]);
    assert_eq!(moved.len(), 2);
    assert!(moved.contains(&(key("c"), 1)) && moved.contains(&(key("b"), -1)));
    let undone = reorder(&mut list, &before);
    assert_eq!(names(&list), ["a", "b", "c"]);
    assert_eq!(undone.len(), 2);
}

#[test]
fn leaving_rows_are_not_sent_to_the_core_and_stay_last() {
    let mut list = rows(&["a", "b", "c"]);
    let leaving: HashSet<_> = [key("a")].into();
    let order = visible_order(&list, &leaving);
    assert_eq!(order, [key("b"), key("c")]);
    let next = move_row(&order, &key("c"), -1).expect("move");
    reorder(&mut list, &next);
    assert_eq!(names(&list), ["c", "b", "a"]);
}

#[test]
fn a_removed_row_stays_until_its_fade_ends_and_survives_snapshots() {
    let mut list = rows(&["a", "b", "c"]);
    list[1].pending = RowPending::Removing;
    let mut leaving = HashSet::new();
    leaving.insert(key("b"));
    // The core no longer lists b.
    let (merged, added) = merge_rows(&list, &leaving, rows(&["a", "c"]));
    assert_eq!(names(&merged), ["a", "c", "b"]);
    assert!(added.is_empty());
    // A late snapshot that still lists b does not duplicate or reorder it.
    let (merged, _) = merge_rows(&merged, &leaving, rows(&["a", "b", "c"]));
    assert_eq!(names(&merged), ["a", "c", "b"]);
    let mut list = merged;
    finish_leaving(&mut list, &mut leaving, &[key("b")]);
    assert_eq!(names(&list), ["a", "c"]);
    assert!(leaving.is_empty());
}

#[test]
fn merging_keeps_row_state_and_reports_new_rows() {
    let mut list = rows(&["a", "b"]);
    list[0].pending = RowPending::Error(AddonError::Core(InstallError::Storage));
    let (merged, added) = merge_rows(&list, &HashSet::new(), rows(&["b", "a", "n"]));
    assert_eq!(names(&merged), ["b", "a", "n"]);
    assert_eq!(added, [key("n")]);
    assert!(matches!(merged[1].pending, RowPending::Error(_)));
}

#[test]
fn focus_after_remove_prefers_the_next_row_then_the_previous() {
    let list = vec![row("a", false), row("b", false), row("c", false)];
    let none = HashSet::new();
    assert_eq!(focus_after_remove(&list, &none, &key("a")), Some(key("b")));
    assert_eq!(focus_after_remove(&list, &none, &key("b")), Some(key("c")));
    assert_eq!(focus_after_remove(&list, &none, &key("c")), Some(key("b")));
    // Built-in rows have no Remove button.
    let list = vec![row("a", false), row("b", true), row("c", false)];
    assert_eq!(focus_after_remove(&list, &none, &key("a")), Some(key("c")));
    let list = vec![row("a", true), row("b", false)];
    assert_eq!(focus_after_remove(&list, &none, &key("b")), None);
    assert_eq!(focus_after_remove(&list, &none, &key("x")), None);
    // Rows already leaving are not candidates.
    let list = rows(&["a", "b", "c"]);
    let leaving: HashSet<_> = [key("b")].into();
    assert_eq!(
        focus_after_remove(&list, &leaving, &key("a")),
        Some(key("c"))
    );
}

#[test]
fn a_stale_meta_failure_cannot_revert_a_newer_choice() {
    let choices = vec![choice("one"), choice("two"), choice("three")];
    let mut meta = MetaState::default();
    meta.take(choices.clone(), Some(key("one")));
    assert_eq!(meta.selected, Some(key("one")));
    let first = meta.choose(Some(key("two"))).expect("save");
    let second = meta.choose(Some(key("three"))).expect("save");
    // The earlier request fails late; it is ignored outright.
    let failure = Err(AddonError::Core(InstallError::ApiPush));
    assert!(!meta.reply(first, failure, Some((choices.clone(), Some(key("one"))))));
    assert_eq!(meta.selected, Some(key("three")));
    assert_eq!(meta.error, None);
    assert!(meta.reply(second, Ok(()), Some((choices, Some(key("three"))))));
    assert_eq!(meta.selected, Some(key("three")));
}

#[test]
fn a_meta_failure_resyncs_from_the_core_snapshot() {
    let choices = vec![choice("one"), choice("two")];
    let mut meta = MetaState::default();
    meta.take(choices.clone(), Some(key("one")));
    let id = meta.choose(Some(key("two"))).expect("save");
    // While the save runs, an unrelated snapshot must not undo the pick.
    meta.take(choices.clone(), Some(key("one")));
    assert_eq!(meta.selected, Some(key("two")));
    let error = AddonError::Core(InstallError::Storage);
    meta.reply(id, Err(error), Some((choices.clone(), Some(key("one")))));
    assert_eq!(meta.selected, Some(key("one")));
    assert_eq!(meta.error, Some(error));
    // No readable snapshot: fall back to the last saved source.
    let id = meta.choose(Some(key("two"))).expect("save");
    meta.reply(id, Err(error), None);
    assert_eq!(meta.selected, Some(key("one")));
}

#[test]
fn automatic_is_persisted_when_the_saved_source_is_stale() {
    let mut meta = MetaState::default();
    // The core still holds a source that is no longer a choice, so Automatic is shown.
    meta.take(vec![choice("one")], Some(key("gone")));
    assert_eq!(meta.selected, None);
    let id = meta.choose(None).expect("Automatic must still be saved");
    assert!(meta.reply(id, Ok(()), None));
    assert_eq!(meta.choose(None), None, "now both shown and saved");
    assert!(meta.choose(Some(key("one"))).is_some());
}

#[test]
fn only_the_newest_request_is_current() {
    let mut latest = Latest::default();
    let first = latest.next();
    assert!(latest.is_current(first));
    let second = latest.next();
    assert!(!latest.is_current(first) && latest.is_current(second));
}
