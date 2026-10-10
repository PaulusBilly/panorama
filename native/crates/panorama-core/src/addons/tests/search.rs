use super::*;

fn search_addon(host: &str) -> Descriptor {
    addon(
        host,
        host,
        json!([{"id":"find","type":"movie","extra":[{"name":"search","isRequired":true}]}]),
        json!(["meta"]),
        Value::Null,
    )
}

#[tokio::test(start_paused = true)]
async fn search_enriches_results_missing_only_release_info() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [search_addon("search.test")];
    let mut original = movie("custom:year", "Original");
    original.as_object_mut().unwrap().remove("releaseInfo");
    fake.set(
        "search.test",
        "catalog",
        "find",
        Reply::json(json!({"metas":[original]})),
    );
    let mut details = movie("custom:year", "Metadata name");
    details["releaseInfo"] = json!("2025");
    details["poster"] = json!("https://images.test/metadata.jpg");
    fake.set(
        "search.test",
        "meta",
        "custom:year",
        Reply::json(json!({"meta":details})),
    );
    let events = client.search(&addons, "film").collect::<Vec<_>>().await;
    assert!(matches!(&events[0], ResourceEvent::Fresh(items) if items[0].release_info.is_none()));
    let ResourceEvent::Fresh(items) = events.last().unwrap() else {
        panic!("expected enriched results");
    };
    assert_eq!(items[0].release_info.as_deref(), Some("2025"));
    assert_eq!(items[0].name, "Original");
    assert_eq!(
        items[0].poster.as_ref().unwrap().as_str(),
        "https://images.test/poster.jpg"
    );
    assert_eq!(fake.hosts(), ["search.test", "search.test"]);
}

#[tokio::test(start_paused = true)]
async fn search_queries_at_most_four_search_capable_catalogs() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let mut addons = vec![home_addon("no-search.test")];
    addons.extend((0..6).map(|index| search_addon(&format!("search{index}.test"))));
    for index in 0..6 {
        fake.set(
            &format!("search{index}.test"),
            "catalog",
            "find",
            page(&format!("tt{index}"), &format!("Film {index}")).delayed(1),
        );
    }
    let events = client.search(&addons, " movie ").collect::<Vec<_>>().await;
    let ResourceEvent::Fresh(items) = &events[0] else {
        panic!("expected fresh");
    };
    assert_eq!(items.len(), 4);
    assert_eq!(
        fake.hosts(),
        [
            "search0.test",
            "search1.test",
            "search2.test",
            "search3.test"
        ]
    );
    assert_eq!(fake.maximum.load(Ordering::SeqCst), 4);
    assert!(fake.calls.lock().unwrap().iter().all(|(_, path)| {
        path.get_extra_first_value("search").map(String::as_str) == Some("movie")
    }));
}

#[tokio::test(start_paused = true)]
async fn search_fills_first_twenty_thin_results_four_at_a_time() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [search_addon("search.test")];
    let metas = (0..25).map(|index| json!({"id":format!("custom:{index}"),"type":"movie","name":format!("Film {index}")})).collect::<Vec<_>>();
    fake.set(
        "search.test",
        "catalog",
        "find",
        Reply::json(json!({"metas":metas})),
    );
    for index in 0..25 {
        fake.set(
            "search.test",
            "meta",
            &format!("custom:{index}"),
            Reply::json(
                json!({"meta":movie(&format!("custom:{index}"), &format!("Filled {index}"))}),
            )
            .delayed(if index == 0 { 2 } else { 1 }),
        );
    }
    let events = client.search(&addons, "movie").collect::<Vec<_>>().await;
    assert_eq!(fake.maximum.load(Ordering::SeqCst), 4);
    let ResourceEvent::Fresh(items) = events.last().unwrap() else {
        panic!("expected fresh");
    };
    assert_eq!(items.len(), 25);
    assert!(items[..20].iter().all(|film| film.poster.is_some()));
    assert!(items[20..].iter().all(|film| film.poster.is_none()));
    let calls = fake.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(_, path)| path.resource == "meta")
            .count(),
        20
    );
    assert!(
        !calls
            .iter()
            .any(|(_, path)| path.resource == "meta" && path.id == "custom:20")
    );
    assert!(events.len() > 2);
    assert!(events.iter().any(|event| matches!(event, ResourceEvent::Fresh(items) if items[0].poster.is_none() && items[1].poster.is_some())));
}

#[tokio::test(start_paused = true)]
async fn search_merges_in_account_order_deduplicates_and_bounds_query() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [search_addon("first.test"), search_addon("second.test")];
    fake.set(
        "first.test",
        "catalog",
        "find",
        Reply::json(json!({"metas":[movie("tt1","First"), movie("tt2","Two")]})).delayed(2),
    );
    fake.set(
        "second.test",
        "catalog",
        "find",
        Reply::json(json!({"metas":[movie("tt1","Duplicate"), movie("tt3","Three")]})).delayed(1),
    );
    let events = client
        .search(&addons, &format!(" {} ", "é".repeat(110)))
        .collect::<Vec<_>>()
        .await;
    let ResourceEvent::Fresh(items) = &events[0] else {
        panic!("expected fresh");
    };
    assert_eq!(
        items
            .iter()
            .map(|film| film.name.as_str())
            .collect::<Vec<_>>(),
        ["First", "Two", "Three"]
    );
    assert!(
        fake.calls
            .lock()
            .unwrap()
            .iter()
            .all(|(_, path)| path.extra[0].value.chars().count() == 100)
    );
    fake.calls.lock().unwrap().clear();
    assert_eq!(
        client.search(&addons, " \t\n ").collect::<Vec<_>>().await,
        [ResourceEvent::Fresh(vec![])]
    );
    assert!(fake.hosts().is_empty());
}

#[tokio::test(start_paused = true)]
async fn search_offline_and_isolated_failure_and_enrichment_failure() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [search_addon("offline.test")];
    fake.set(
        "offline.test",
        "catalog",
        "find",
        Reply::error(FailureKind::Offline),
    );
    assert_eq!(
        client.search(&addons, "film").collect::<Vec<_>>().await,
        [ResourceEvent::Failed(FailureKind::Offline)]
    );
    fake.set(
        "offline.test",
        "catalog",
        "find",
        Reply::json(json!({"metas":[
            {"id":"custom:0","type":"movie","name":"Missing details"},
            {"id":"custom:1","type":"movie","name":"Fill succeeds"}
        ]})),
    );
    fake.set(
        "offline.test",
        "meta",
        "custom:1",
        Reply::json(json!({"meta":movie("custom:1","Filled")})),
    );
    let events = client.search(&addons, "film").collect::<Vec<_>>().await;
    assert!(
        matches!(events.last(), Some(ResourceEvent::Fresh(items)) if items[1].poster.is_some())
    );
    let combined = [addons[0].clone(), search_addon("down.test")];
    assert!(
        client
            .search(&combined, "film")
            .collect::<Vec<_>>()
            .await
            .iter()
            .any(|event| matches!(event, ResourceEvent::Fresh(items) if items.len() == 2))
    );
}

#[tokio::test(start_paused = true)]
async fn dropping_search_cancels_inflight_requests() {
    let fake = Fake::default();
    let client = client(&fake).await;
    fake.set(
        "slow.test",
        "catalog",
        "find",
        page("tt1", "Slow").delayed(100),
    );
    let stream = client.search(&[search_addon("slow.test")], "film");
    tokio::task::yield_now().await;
    assert_eq!(fake.active.load(Ordering::SeqCst), 1);
    drop(stream);
    tokio::task::yield_now().await;
    assert_eq!(fake.active.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn search_enriches_missing_names_and_delivers_cached_then_fresh_details_in_place() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [search_addon("search.test")];
    fake.set(
        "search.test",
        "meta",
        "custom:1",
        Reply::json(json!({"meta":movie("custom:1","Cached")})),
    );
    client
        .details(&addons, "custom:1", None)
        .collect::<Vec<_>>()
        .await;
    fake.set(
        "search.test",
        "catalog",
        "find",
        Reply::json(json!({"metas":[
            {"id":"custom:1","type":"movie","name":""},
            {"id":"custom:2","type":"movie","name":""},
            movie("custom:3", "Original")
        ]})),
    );
    let mut refreshed = movie("custom:1", "Fresh");
    refreshed["poster"] = json!("https://images.test/fresh.jpg");
    fake.set(
        "search.test",
        "meta",
        "custom:1",
        Reply::json(json!({"meta":refreshed})).delayed(3),
    );
    let started = tokio::time::Instant::now();
    let mut results = client.search(&addons, "film");
    assert!(
        matches!(results.next().await, Some(ResourceEvent::Fresh(items)) if items.len() == 1 && items[0].id == "custom:3")
    );
    assert!(
        matches!(results.next().await, Some(ResourceEvent::Fresh(items)) if items.len() == 2 && items[0].name == "Cached" && items[1].name == "Original")
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    let remaining = results.collect::<Vec<_>>().await;
    assert!(
        matches!(remaining.last(), Some(ResourceEvent::Fresh(items)) if items.len() == 2 && items[0].name == "Fresh" && items[0].poster.as_ref().unwrap().path() == "/fresh.jpg")
    );
}

#[tokio::test(start_paused = true)]
async fn search_with_no_capable_catalogs_is_empty_without_requests() {
    let fake = Fake::default();
    let client = client(&fake).await;
    assert_eq!(
        client
            .search(&[home_addon("no-search.test")], "film")
            .collect::<Vec<_>>()
            .await,
        [ResourceEvent::Fresh(vec![])]
    );
    assert!(fake.hosts().is_empty());
}
