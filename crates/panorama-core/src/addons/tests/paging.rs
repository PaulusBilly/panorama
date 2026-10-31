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
async fn catalogs_hide_unreleased_films_without_moving_the_paging_position() {
    let fake = Fake::default();
    let addons = [addon(
        "upcoming.test",
        "upcoming",
        json!([{"id":"top","type":"movie","extra":[{"name":"skip","isRequired":false}]}]),
        json!(["catalog"]),
        Value::Null,
    )];
    let dated = |id, name, field: &str, value: &str| {
        let mut film = movie(id, name);
        film[field] = json!(value);
        film
    };
    fake.set(
        "upcoming.test",
        "catalog",
        "top",
        Reply::json(json!({"metas":[
            dated("tt1", "Out", "released", "2001-05-01T00:00:00.000Z"),
            dated("tt2", "Dated", "released", "2999-05-01T00:00:00.000Z"),
            dated("tt3", "Next year", "releaseInfo", "2999"),
            dated("tt4", "This era", "releaseInfo", "2001-2003"),
            dated("tt5", "Unknown", "releaseInfo", "TBA"),
            movie("tt6", "Year only"),
        ]})),
    );
    let client = client(&fake).await;
    let events = client.catalog(&addons, None).collect::<Vec<_>>().await;
    let page = fresh_page(&events);
    let names = page.items.iter().map(|film| film.name.as_str());
    assert_eq!(names.collect::<Vec<_>>(), ["Out", "This era", "Year only"]);
    assert_eq!(page.next_skip, 6);
    assert!(page.has_more);
}

#[tokio::test(start_paused = true)]
async fn a_catalog_that_sends_no_dates_keeps_its_films() {
    let fake = Fake::default();
    let addons = [home_addon("nodates.test")];
    let bare = |id, name| json!({"id":id,"type":"movie","name":name});
    fake.set(
        "nodates.test",
        "catalog",
        "top",
        Reply::json(json!({"metas":[bare("tt1", "One"), bare("tt2", "Two")]})),
    );
    let client = client(&fake).await;
    let events = client.catalog(&addons, None).collect::<Vec<_>>().await;
    assert_eq!(fresh_page(&events).items.len(), 2);
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
