use super::model::*;
use panorama_core::addons::{AddonKey, CatalogRef, manage::InstallError};
use std::time::{Duration, Instant};

fn key(name: &str) -> AddonKey {
    serde_json::from_value(name.into()).expect("addon key")
}

fn card(name: &str) -> PreviewCard {
    PreviewCard {
        name: name.into(),
        version: "1.0.0".into(),
        description: None,
        logo: None,
        capabilities: vec![],
        types: vec![],
    }
}

#[test]
fn happy_path_clears_to_idle_and_names_the_addon() {
    let now = Instant::now();
    let mut flow = InstallFlow::<u8>::Idle;
    assert!(!flow.submit(0));
    assert!(flow.submit(30));
    assert!(!flow.submit(30));
    flow.previewed(Ok((card("Example"), 7)), now);
    assert_eq!(flow.confirm(now), Some(7));
    assert!(flow.busy());
    assert_eq!(flow.installed().as_deref(), Some("Example"));
    assert!(matches!(flow, InstallFlow::Idle));
}

#[test]
fn cancel_during_previewing_drops_the_late_result() {
    let now = Instant::now();
    let mut flow = InstallFlow::<u8>::Idle;
    flow.submit(10);
    assert!(flow.cancel());
    flow.previewed(Ok((card("Late"), 1)), now);
    assert!(matches!(flow, InstallFlow::Idle));
    flow.previewed(Err(InstallError::Network.into()), now);
    assert!(matches!(flow, InstallFlow::Idle));
}

#[test]
fn preview_expires_after_ten_minutes() {
    let now = Instant::now();
    let mut flow = InstallFlow::<u8>::Idle;
    flow.submit(10);
    flow.previewed(Ok((card("A"), 1)), now);
    flow.expire(now + PREVIEW_TTL - Duration::from_secs(1));
    assert!(matches!(flow, InstallFlow::Preview { .. }));
    assert_eq!(flow.confirm(now + PREVIEW_TTL), None);
    assert!(matches!(flow, InstallFlow::Failed(AddonError::Expired)));
    assert!(flow.cancel());
}

#[test]
fn failures_only_apply_to_work_in_flight() {
    let mut flow = InstallFlow::<u8>::Idle;
    flow.failed(InstallError::Timeout.into());
    assert!(matches!(flow, InstallFlow::Idle));
    flow.submit(5);
    flow.failed(InstallError::Timeout.into());
    assert!(matches!(
        flow,
        InstallFlow::Failed(AddonError::Core(InstallError::Timeout))
    ));
    assert!(flow.submit(5), "a failed flow can be retried");
    let now = Instant::now();
    flow.previewed(Ok((card("A"), 1)), now);
    flow.confirm(now);
    assert!(!flow.cancel(), "an install in flight cannot be cancelled");
    assert!(!flow.submit(5));
    flow.failed(InstallError::ApiPush.into());
    assert!(matches!(flow, InstallFlow::Failed(_)));
}

#[test]
fn move_row_swaps_neighbours_and_stops_at_edges() {
    let order = [key("a#1"), key("b#1"), key("c#1")];
    assert_eq!(
        move_row(&order, &order[1], -1),
        Some(vec![order[1].clone(), order[0].clone(), order[2].clone()])
    );
    assert_eq!(
        move_row(&order, &order[1], 1),
        Some(vec![order[0].clone(), order[2].clone(), order[1].clone()])
    );
    assert_eq!(move_row(&order, &order[0], -1), None);
    assert_eq!(move_row(&order, &order[2], 1), None);
    assert_eq!(move_row(&order, &key("x#1"), 1), None);
    assert_eq!(move_row(&order, &order[0], 0), None);
}

#[test]
fn every_error_has_distinct_plain_copy() {
    use InstallError::*;
    let all = [
        InvalidUrl,
        InvalidManifest,
        TooLarge,
        Network,
        Timeout,
        SignedOut,
        Protected,
        ImpersonatesProtected,
        InvalidOrder,
        InvalidSelection,
        InvalidToken,
        ConfigurationRequired,
        Core,
        ApiPush,
        Storage,
    ];
    let mut copy: Vec<_> = all.iter().map(|e| error_copy(&(*e).into())).collect();
    copy.push(error_copy(&AddonError::Expired));
    for line in &copy {
        assert!(line.ends_with('.') && !line.contains("Error"), "{line}");
    }
    copy.sort_unstable();
    copy.dedup();
    assert_eq!(copy.len(), all.len() + 1);
    assert_eq!(
        error_copy(&Protected.into()),
        "Built-in addons can't be removed."
    );
}

#[test]
fn catalog_groups_follow_account_order() {
    let addons = [
        (key("one#1"), "One".to_string()),
        (key("two#1"), "Two".to_string()),
    ];
    let catalog = |addon: &AddonKey, id: &str| CatalogRef {
        addon: addon.clone(),
        catalog_id: id.into(),
        name: id.into(),
    };
    let catalogs = [
        catalog(&addons[1].0, "b"),
        catalog(&addons[0].0, "a"),
        catalog(&addons[1].0, "c"),
    ];
    let groups = catalog_groups(&addons, &catalogs);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].addon, "Two");
    assert_eq!(groups[0].catalogs.len(), 2);
    assert_eq!(groups[1].addon, "One");
    let fallback = catalog_groups(&[], &[catalog(&key("cinemeta#1"), "top")]);
    assert_eq!(fallback[0].addon, "Cinemeta");
    assert!(catalog_groups(&addons, &[]).is_empty());
}

#[test]
fn saved_meta_source_falls_back_to_automatic_when_gone() {
    let choices = [MetaChoice {
        key: key("cinemeta#1"),
        name: "Cinemeta".into(),
    }];
    assert_eq!(
        meta_selection(Some(key("cinemeta#1")), &choices),
        Some(key("cinemeta#1"))
    );
    assert_eq!(meta_selection(Some(key("removed#1")), &choices), None);
    assert_eq!(meta_selection(None, &choices), None);
    assert_eq!(meta_selection(Some(key("cinemeta#1")), &[]), None);
}

#[test]
fn radio_keys_wrap() {
    assert_eq!(radio_step(3, 0, -1), 2);
    assert_eq!(radio_step(3, 2, 1), 0);
    assert_eq!(radio_step(3, 1, 1), 2);
    assert_eq!(radio_step(0, 0, 1), 0);
}

#[test]
fn sections_wait_for_sign_in_and_a_loaded_list() {
    assert_eq!(screen(false, false, false), Screen::SignedOut);
    assert_eq!(screen(false, true, true), Screen::SignedOut);
    assert_eq!(screen(true, true, false), Screen::Loading);
    assert_eq!(screen(true, false, true), Screen::Failed);
    assert_eq!(screen(true, false, false), Screen::Ready);
}

#[test]
fn untrusted_text_is_flattened_and_capped() {
    assert_eq!(display_text("  a\u{0}b\nc  ", 80), "ab c");
    assert_eq!(display_text(&"x".repeat(200), NAME_CAP).len(), NAME_CAP);
}

#[test]
fn capabilities_keep_a_fixed_order() {
    let resources: Vec<String> = ["stream", "catalog", "other", "meta"]
        .map(String::from)
        .to_vec();
    assert_eq!(
        capabilities(&resources),
        [
            Capability::Catalogs,
            Capability::Details,
            Capability::Streams
        ]
    );
    let types: Vec<String> = ["series", "movie", "tv"].map(String::from).to_vec();
    assert_eq!(media_types(&types), ["Movies", "Series"]);
}

#[test]
fn format_characters_are_stripped_from_untrusted_text() {
    // Bidi override and isolates, zero-width and word-joiner marks, BOM, soft hyphen, tag.
    let hostile =
        "ab\u{202E}c\u{2066}d\u{2069}e\u{200B}f\u{200F}g\u{2060}h\u{FEFF}i\u{00AD}j\u{E0041}k";
    assert_eq!(display_text(hostile, 80), "abcdefghijk");
    assert_eq!(display_text("\u{200D}\u{200C}", 80), "");
    assert_eq!(
        display_text("Caf\u{e9} \u{65e5}\u{672c}", 80),
        "Caf\u{e9} \u{65e5}\u{672c}"
    );
}

#[test]
fn an_edit_discards_a_preview_but_not_work_in_flight() {
    let now = Instant::now();
    let mut flow = InstallFlow::<u8>::Idle;
    assert!(!flow.edited());
    flow.submit(10);
    assert!(!flow.edited(), "the field is locked while fetching");
    assert!(flow.busy());
    flow.previewed(Ok((card("Old"), 1)), now);
    assert!(flow.edited());
    assert!(matches!(flow, InstallFlow::Idle));
    assert_eq!(
        flow.confirm(now),
        None,
        "the old manifest can no longer be installed"
    );
    flow.submit(10);
    flow.previewed(Ok((card("A"), 1)), now);
    flow.confirm(now);
    assert!(!flow.edited());
    assert!(matches!(flow, InstallFlow::Installing { .. }));
    flow.failed(InstallError::Network.into());
    assert!(flow.edited());
    assert!(matches!(flow, InstallFlow::Idle));
}

#[test]
fn debug_output_never_carries_untrusted_text() {
    let secret = card("SECRET name");
    let flow: InstallFlow<u8> = InstallFlow::Installing {
        card: secret.clone(),
    };
    assert!(!format!("{flow:?}").contains("SECRET"));
    assert!(!format!("{secret:?}").contains("SECRET"));
}
