use super::*;

#[tokio::test(start_paused = true)]
async fn unknown_film_shows_details_with_no_sources_state() {
    let fake = Fake::default();
    let addons = [addon(
        "meta.test",
        "meta",
        json!([]),
        json!(["meta"]),
        json!(["opaque:"]),
    )];
    fake.set(
        "meta.test",
        "meta",
        "opaque:123",
        Reply::json(json!({"meta":movie("opaque:123","Unknown film")})),
    );
    let client = client(&fake).await;
    let events = client
        .details(&addons, "opaque:123", None)
        .collect::<Vec<_>>()
        .await;
    assert!(matches!(&events[0], ResourceEvent::Fresh(film) if film.name == "Unknown film"));
    assert_eq!(
        client
            .streams(&addons, "opaque:123")
            .collect::<Vec<_>>()
            .await,
        [StreamsEvent::NoSources]
    );
}

#[tokio::test(start_paused = true)]
async fn details_chain_prefers_meta_addon_then_matching_addons_then_cinemeta() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [
        home_addon("first.test"),
        home_addon("preferred.test"),
        addon(
            "skip.test",
            "skip",
            json!([]),
            json!([{"name":"meta","types":["movie"],"idPrefixes":["kitsu:"]}]),
            Value::Null,
        ),
    ];
    client
        .write(
            Key::pref("metaAddon").unwrap(),
            &client.addon_key(&addons[1]).unwrap(),
        )
        .await
        .unwrap();
    fake.set(
        "preferred.test",
        "meta",
        "tt1",
        Reply::json(json!({"meta":movie("wrong-id", "Wrong")})),
    );
    fake.set(
        "first.test",
        "meta",
        "tt1",
        Reply::json(json!({"metas":[]})),
    );
    fake.set(
        "v3-cinemeta.strem.io",
        "meta",
        "tt1",
        Reply::json(json!({"meta":movie("tt1", "Final")})),
    );
    let events = client
        .details(&addons, "tt1", None)
        .collect::<Vec<_>>()
        .await;
    assert!(matches!(&events[0], ResourceEvent::Fresh(film) if film.name == "Final"));
    assert_eq!(
        fake.hosts(),
        ["preferred.test", "first.test", "v3-cinemeta.strem.io"]
    );
    fake.calls.lock().unwrap().clear();
    fake.set(
        "first.test",
        "meta",
        "tt2",
        Reply::json(json!({"meta":movie("tt2","Explicit")})),
    );
    client
        .details(&addons, "tt2", Some(client.addon_key(&addons[0]).unwrap()))
        .collect::<Vec<_>>()
        .await;
    assert_eq!(fake.hosts(), ["first.test"]);
}

#[tokio::test(start_paused = true)]
async fn streams_grouped_by_addon_in_account_order_with_failures_isolated() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [
        home_addon("slow.test"),
        home_addon("ready.test"),
        home_addon("empty.test"),
        home_addon("down.test"),
    ];
    fake.set(
        "slow.test",
        "stream",
        "tt1",
        Reply::json(json!({"streams":[]})).delayed(16),
    );
    fake.set("ready.test", "stream", "tt1", Reply::json(json!({"streams":[{"url":"https://video.test/a","name":"Direct","title":"Movie","behaviorHints":{"notWebReady":true}}, {"infoHash":"0123456789012345678901234567890123456789","fileIdx":3}, {"externalUrl":"https://external.test"}]})).delayed(1));
    fake.set(
        "empty.test",
        "stream",
        "tt1",
        Reply::json(json!({"streams":[]})).delayed(1),
    );
    fake.set(
        "down.test",
        "stream",
        "tt1",
        Reply::error(FailureKind::Network).delayed(1),
    );
    let events = client.streams(&addons, "tt1").collect::<Vec<_>>().await;
    let StreamsEvent::Groups(groups) = &events[0] else {
        panic!("expected groups");
    };
    assert_eq!(
        groups
            .iter()
            .map(|group| group.name.as_str())
            .collect::<Vec<_>>(),
        ["slow.test", "ready.test", "empty.test", "down.test"]
    );
    assert_eq!(groups[0].state, StreamState::Failed(FailureKind::Timeout));
    let StreamState::Ready(sources) = &groups[1].state else {
        panic!("expected ready");
    };
    assert_eq!(sources.len(), 3);
    assert_eq!(sources[0].title.as_deref(), Some("Movie"));
    assert!(sources[0].stream.behavior_hints.not_web_ready);
    assert_eq!(groups[2].state, StreamState::Empty);
    assert_eq!(groups[3].state, StreamState::Failed(FailureKind::Network));
    assert_eq!(fake.maximum.load(Ordering::SeqCst), 4);
    assert_eq!(fake.active.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn details_offline_cache_and_removed_installation_and_non_imdb_fallback() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [home_addon("meta.test")];
    fake.set(
        "meta.test",
        "meta",
        "custom:1",
        Reply::json(json!({"meta":movie("custom:1", "Cached")})),
    );
    client
        .details(&addons, "custom:1", None)
        .collect::<Vec<_>>()
        .await;
    fake.set(
        "meta.test",
        "meta",
        "custom:1",
        Reply::error(FailureKind::Offline),
    );
    let events = client
        .details(&addons, "custom:1", None)
        .collect::<Vec<_>>()
        .await;
    assert!(matches!(&events[0], ResourceEvent::CacheHit(film) if film.name == "Cached"));
    assert_eq!(events[1], ResourceEvent::Failed(FailureKind::Offline));
    assert_eq!(
        client
            .details(&[], "custom:1", None)
            .collect::<Vec<_>>()
            .await,
        [ResourceEvent::Failed(FailureKind::InvalidResponse)]
    );
    fake.set(
        "v3-cinemeta.strem.io",
        "meta",
        "tt1",
        Reply::error(FailureKind::Offline),
    );
    assert_eq!(
        client.details(&[], "tt1", None).collect::<Vec<_>>().await,
        [ResourceEvent::Failed(FailureKind::Offline)]
    );
}

#[tokio::test(start_paused = true)]
async fn streams_offline_are_uncached_and_resource_types_and_prefixes_are_respected() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [
        home_addon("offline.test"),
        addon(
            "series.test",
            "series",
            json!([]),
            json!([{"name":"stream","types":["series"]}]),
            Value::Null,
        ),
        addon(
            "other.test",
            "other",
            json!([]),
            json!(["stream"]),
            json!(["other:"]),
        ),
    ];
    fake.set(
        "offline.test",
        "stream",
        "tt1",
        Reply::error(FailureKind::Offline),
    );
    let events = client.streams(&addons, "tt1").collect::<Vec<_>>().await;
    assert_eq!(events, [StreamsEvent::Failed(FailureKind::Offline)]);
    assert_eq!(fake.hosts(), ["offline.test"]);
    assert!(
        client
            .store
            .get(&Key::core("streams").unwrap())
            .unwrap()
            .is_none()
    );
}

#[tokio::test(start_paused = true)]
async fn full_resource_declarations_inherit_manifest_constraints_and_preferred_meta_is_first() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let mut inherited = addon(
        "inherited.test",
        "inherited",
        json!([]),
        json!([{"name":"meta"},{"name":"stream"}]),
        json!(["custom:"]),
    );
    let preferred = addon(
        "preferred.test",
        "preferred",
        json!([]),
        json!(["meta"]),
        json!(["other:"]),
    );
    fake.set(
        "preferred.test",
        "meta",
        "custom:1",
        Reply::error(FailureKind::Network),
    );
    fake.set(
        "inherited.test",
        "meta",
        "custom:1",
        Reply::json(json!({"meta":movie("custom:1","Inherited")})),
    );
    let addons = [inherited.clone(), preferred.clone()];
    let events = client
        .details(
            &addons,
            "custom:1",
            Some(client.addon_key(&preferred).unwrap()),
        )
        .collect::<Vec<_>>()
        .await;
    assert!(matches!(&events[..], [ResourceEvent::Fresh(film)] if film.name == "Inherited"));
    assert_eq!(fake.hosts(), ["preferred.test", "inherited.test"]);
    assert_eq!(
        client
            .streams(std::slice::from_ref(&inherited), "other:1")
            .collect::<Vec<_>>()
            .await,
        [StreamsEvent::NoSources]
    );
    let mut core = inherited.as_core().clone();
    core.manifest.types = vec!["series".into()];
    inherited = Descriptor::from_core(core);
    assert_eq!(
        client
            .streams(&[inherited], "custom:1")
            .collect::<Vec<_>>()
            .await,
        [StreamsEvent::NoSources]
    );
}
