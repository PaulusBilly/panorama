use super::*;

#[tokio::test(start_paused = true)]
async fn home_falls_back_when_first_addon_is_slow_down_or_junk() {
    for bad in [
        page("tt1", "Late").delayed(9),
        Reply::error(FailureKind::Network),
        Reply {
            delay: Duration::ZERO,
            value: Ok(b"junk".to_vec()),
        },
    ] {
        let fake = Fake::default();
        fake.set("first.test", "catalog", "top", bad);
        fake.set("second.test", "catalog", "top", page("tt2", "Works"));
        let client = client(&fake).await;
        let events = client
            .catalog(&[home_addon("first.test"), home_addon("second.test")], None)
            .collect::<Vec<_>>()
            .await;
        assert_eq!(fresh_page(&events).items[0].id, "tt2");
        assert_eq!(fake.hosts(), ["first.test", "second.test"]);
        assert_eq!(fake.active.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn home_never_blank_while_cinemeta_works() {
    for bad in [
        json!({"streams":[]}),
        json!({"metas":[]}),
        json!({"metas":[{"id":"tt1","type":"movie","name":""}, {"name":"Missing ID"}]}),
    ] {
        let fake = Fake::default();
        fake.set("first.test", "catalog", "top", Reply::json(bad));
        fake.set(
            "v3-cinemeta.strem.io",
            "catalog",
            "top",
            page("tt2", "Cinemeta"),
        );
        let client = client(&fake).await;
        let events = client
            .catalog(&[home_addon("first.test")], None)
            .collect::<Vec<_>>()
            .await;
        assert_eq!(fresh_page(&events).items[0].name, "Cinemeta");
    }
}

#[tokio::test(start_paused = true)]
async fn offline_launch_shows_cached_catalog_then_offline_error() {
    let fake = Fake::default();
    fake.set("first.test", "catalog", "top", page("tt1", "Cached"));
    let client = client(&fake).await;
    let addons = [home_addon("first.test")];
    let original = client.catalog(&addons, None).collect::<Vec<_>>().await;
    fake.set(
        "first.test",
        "catalog",
        "top",
        Reply::error(FailureKind::Offline),
    );
    let offline = client.catalog(&addons, None).collect::<Vec<_>>().await;
    assert_eq!(
        offline,
        [
            ResourceEvent::CacheHit(fresh_page(&original).clone()),
            ResourceEvent::Failed(FailureKind::Offline)
        ]
    );
    fake.set("first.test", "catalog", "top", page("tt1", "Refreshed"));
    let retry = client.catalog(&addons, None).collect::<Vec<_>>().await;
    assert!(matches!(&retry[0], ResourceEvent::CacheHit(_)));
    assert_eq!(fresh_page(&retry).items[0].name, "Refreshed");
    let replaced = client.catalog(&addons, None).collect::<Vec<_>>().await;
    assert!(
        matches!(&replaced[0], ResourceEvent::CacheHit(page) if page.items[0].name == "Refreshed")
    );
}

#[tokio::test(start_paused = true)]
async fn offline_launch_with_empty_cache_reports_offline_only() {
    let fake = Fake::default();
    fake.set(
        "v3-cinemeta.strem.io",
        "catalog",
        "top",
        Reply::error(FailureKind::Offline),
    );
    let client = client(&fake).await;
    assert_eq!(
        client.catalog(&[], None).collect::<Vec<_>>().await,
        [ResourceEvent::Failed(FailureKind::Offline)]
    );
}

#[tokio::test(start_paused = true)]
async fn removed_remembered_catalog_falls_back_to_first() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let remembered = client.catalogs(&[home_addon("removed.test")]).remove(0);
    fake.set("first.test", "catalog", "top", page("tt1", "First"));
    let events = client
        .catalog(&[home_addon("first.test")], Some(remembered))
        .collect::<Vec<_>>()
        .await;
    assert_eq!(fresh_page(&events).items[0].name, "First");
    assert_eq!(fake.hosts(), ["first.test"]);
}

#[tokio::test(start_paused = true)]
async fn catalogs_keep_account_order_and_exclude_required_extras_and_other_types() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let first = addon(
        "first.test",
        "first",
        json!([
            {"id":"one","type":"movie","name":"One"},
            {"id":"required","type":"movie","extra":[{"name":"genre","isRequired":true}]},
            {"id":"series","type":"series"}, {"id":"two","type":"movie"}
        ]),
        json!([]),
        Value::Null,
    );
    let catalogs = client.catalogs(&[first, home_addon("second.test")]);
    assert_eq!(
        catalogs
            .iter()
            .map(|catalog| catalog.catalog_id.as_str())
            .collect::<Vec<_>>(),
        ["one", "two", "top"]
    );
    assert_eq!(client.catalogs(&[])[0].catalog_id, "top");
}

#[tokio::test(start_paused = true)]
async fn remembered_catalog_is_tried_first_and_fallback_cache_is_available_offline() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [home_addon("first.test"), home_addon("second.test")];
    let selected = client.catalogs(&addons)[1].clone();
    fake.set("second.test", "catalog", "top", page("tt2", "Second"));
    let events = client
        .catalog(&addons, Some(selected))
        .collect::<Vec<_>>()
        .await;
    assert_eq!(fresh_page(&events).items[0].id, "tt2");
    assert_eq!(fake.hosts(), ["second.test"]);
    fake.set(
        "second.test",
        "catalog",
        "top",
        Reply::error(FailureKind::Offline),
    );
    fake.set(
        "first.test",
        "catalog",
        "top",
        Reply::error(FailureKind::Offline),
    );
    let events = client.catalog(&addons, None).collect::<Vec<_>>().await;
    assert!(matches!(events[0], ResourceEvent::CacheHit(_)));
    assert_eq!(events[1], ResourceEvent::Failed(FailureKind::Offline));
}

#[tokio::test(start_paused = true)]
async fn screen_calls_run_without_tokio_context_on_the_callers_thread() {
    let fake = Fake::default();
    let client = client(&fake).await;
    fake.set("screen.test", "catalog", "top", page("custom:1", "Film"));
    fake.set(
        "screen.test",
        "meta",
        "custom:1",
        Reply::json(json!({"meta":movie("custom:1","Film")})),
    );
    let (catalog, details, streams, search) = std::thread::spawn(move || {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let addons = [home_addon("screen.test")];
        assert_eq!(client.catalogs(&addons).len(), 1);
        (
            client.catalog(&addons, None),
            client.details(&addons, "custom:1", None),
            client.streams(&addons, "custom:1"),
            client.search(&addons, ""),
        )
    })
    .join()
    .unwrap();
    assert_eq!(
        fresh_page(&catalog.collect::<Vec<_>>().await).items[0].id,
        "custom:1"
    );
    assert!(
        matches!(&details.collect::<Vec<_>>().await[..], [ResourceEvent::Fresh(film)] if film.name == "Film")
    );
    assert!(
        matches!(&streams.collect::<Vec<_>>().await[..], [StreamsEvent::Groups(groups)] if groups.len() == 1)
    );
    assert_eq!(
        search.collect::<Vec<_>>().await,
        [ResourceEvent::Fresh(vec![])]
    );
}
