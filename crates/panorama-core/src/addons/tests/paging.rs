use super::*;

#[tokio::test(start_paused = true)]
async fn paging_uses_raw_count_and_keeps_the_first_page_cache() {
    let fake = Fake::default();
    let addons = [addon(
        "paging.test",
        "paging",
        json!([{"id":"top","type":"movie","extra":[{"name":"skip","isRequired":false}]}]),
        json!(["catalog"]),
        Value::Null,
    )];
    fake.set(
        "paging.test",
        "catalog",
        "top",
        Reply::json(json!({"metas":[movie("tt1","First"),movie("tt2","")]})),
    );
    let client = client(&fake).await;
    let first = client.catalog(&addons, None).collect::<Vec<_>>().await;
    let first = fresh_page(&first).clone();
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.next_skip, 2);
    assert!(first.has_more);
    fake.set("paging.test", "catalog", "top", page("tt3", "Next"));
    let next = client
        .catalog_next(&addons, first.catalog.clone(), first.next_skip)
        .collect::<Vec<_>>()
        .await;
    assert_eq!(fresh_page(&next).items[0].name, "Next");
    assert_eq!(fresh_page(&next).next_skip, 3);
    assert_eq!(
        fake.calls.lock().unwrap().last().unwrap().1.extra[0].value,
        "2"
    );
    fake.set(
        "paging.test",
        "catalog",
        "top",
        Reply::error(FailureKind::Offline),
    );
    let offline = client.catalog(&addons, None).collect::<Vec<_>>().await;
    assert_eq!(offline[0], ResourceEvent::CacheHit(first.clone()));
    fake.set(
        "paging.test",
        "catalog",
        "top",
        Reply::json(json!({"metas":[]})),
    );
    let empty = client
        .catalog_next(&addons, first.catalog, 3)
        .collect::<Vec<_>>()
        .await;
    assert!(!fresh_page(&empty).has_more);
    assert_eq!(fresh_page(&empty).next_skip, 3);
}

#[tokio::test(start_paused = true)]
async fn preview_display_fields_survive_core_parsing_and_are_bounded() {
    let fake = Fake::default();
    let mut film = movie("tt1", "Display");
    film["director"] = json!(["Charlotte Wells"]);
    film["country"] = json!("Japan");
    fake.set(
        "display.test",
        "catalog",
        "top",
        Reply::json(json!({"metas":[film]})),
    );
    let client = client(&fake).await;
    let events = client
        .catalog(&[home_addon("display.test")], None)
        .collect::<Vec<_>>()
        .await;
    let film = &fresh_page(&events).items[0];
    assert_eq!(film.director, ["Charlotte Wells"]);
    assert_eq!(film.origin_country.as_deref(), Some("Japan"));
    let long = "x".repeat(1000);
    let response=super::super::sanitize::parse(TransportResponse::Bytes(serde_json::to_vec(&json!({"metas":[{"id":"tt2","type":"movie","name":"Bounded","director":long,"country":long}]})).unwrap())).unwrap();
    let films = super::super::sanitize::films(response, false).unwrap();
    assert_eq!(films[0].director[0].len(), 300);
    assert_eq!(films[0].origin_country.as_ref().unwrap().len(), 300);
}
