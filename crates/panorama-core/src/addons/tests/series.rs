use super::*;

fn typed_addon(host: &str, types: Value, catalogs: Value, resources: Value) -> Descriptor {
    Descriptor::from_core(
        serde_json::from_value(json!({
            "transportUrl": format!("https://{host}/manifest.json"),
            "manifest": {"id": host, "version": "1.0.0", "name": host, "types": types,
                "resources": resources, "catalogs": catalogs}
        }))
        .unwrap(),
    )
}

fn kitsu() -> Descriptor {
    typed_addon(
        "kitsu.test",
        json!(["anime", "movie", "series"]),
        json!([{"id":"kitsu-anime-trending","type":"anime","name":"Trending"}]),
        json!(["catalog", "meta", "stream", "subtitles"]),
    )
}

fn requests(fake: &Fake) -> Vec<(String, String, String)> {
    let calls = fake.calls.lock().unwrap();
    calls
        .iter()
        .map(|(_, path)| (path.resource.clone(), path.r#type.clone(), path.id.clone()))
        .collect()
}

fn show(id: &str, name: &str, kind: &str) -> Value {
    json!({"id":id,"type":kind,"name":name,"poster":"https://images.test/poster.jpg","releaseInfo":"2026–"})
}

fn video(id: &str, season: u32, episode: u32) -> Value {
    json!({"id":id,"title":format!("Episode {episode}"),"season":season,"episode":episode,
        "released":"2024-03-01T00:00:00.000Z","thumbnail":"https://images.test/still.jpg"})
}

#[tokio::test(start_paused = true)]
async fn series_meta_keeps_episodes_in_watch_order_with_specials_last() {
    let fake = Fake::default();
    let addons = [typed_addon(
        "meta.test",
        json!(["series"]),
        json!([]),
        json!(["meta"]),
    )];
    let mut broken = video("bad id", 1, 9);
    broken["thumbnail"] = json!("http://images.test/insecure.jpg");
    fake.set(
        "meta.test",
        "meta",
        "tt1",
        Reply::json(json!({"meta": {
            "id":"tt1","type":"series","name":"Carrie","releaseInfo":"2026–",
            "videos":[
                video("tt1:0:1", 0, 1),
                video("tt1:2:1", 2, 1),
                video("tt1:1:2", 1, 2),
                broken,
                video("tt1:1:1", 1, 1),
            ]
        }})),
    );
    let client = client(&fake).await;
    let events = client
        .details(&addons, Kind::Series, "tt1", None)
        .collect::<Vec<_>>()
        .await;
    let ResourceEvent::Fresh(film) = &events[0] else {
        panic!("expected fresh details, got {events:?}");
    };
    assert_eq!(film.kind, Kind::Series);
    let order = film
        .episodes
        .iter()
        .map(|episode| (episode.id.as_str(), episode.season, episode.number))
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        [
            ("tt1:1:1", 1, 1),
            ("tt1:1:2", 1, 2),
            ("tt1:2:1", 2, 1),
            ("tt1:0:1", 0, 1),
        ]
    );
    assert_eq!(film.episodes[0].title, "Episode 1");
    assert_eq!(film.episodes[0].released.as_deref(), Some("2024-03-01"));
    assert_eq!(
        film.episodes[0].thumbnail.as_ref().map(|url| url.as_str()),
        Some("https://images.test/still.jpg")
    );
    assert_eq!(fake.calls.lock().unwrap()[0].1.r#type, "series");
}

#[tokio::test(start_paused = true)]
async fn videos_without_series_info_are_numbered_by_position_in_season_one() {
    let fake = Fake::default();
    let addons = [typed_addon(
        "meta.test",
        json!(["series"]),
        json!([]),
        json!(["meta"]),
    )];
    fake.set(
        "meta.test",
        "meta",
        "custom:1",
        Reply::json(json!({"meta": {
            "id":"custom:1","type":"series","name":"Plain",
            "videos":[{"id":"custom:1:a","title":"First"},{"id":"custom:1:b","title":"Second"}]
        }})),
    );
    let client = client(&fake).await;
    let events = client
        .details(&addons, Kind::Series, "custom:1", None)
        .collect::<Vec<_>>()
        .await;
    let ResourceEvent::Fresh(film) = &events[0] else {
        panic!("expected fresh details, got {events:?}");
    };
    let numbered = film
        .episodes
        .iter()
        .map(|episode| (episode.id.as_str(), episode.season, episode.number))
        .collect::<Vec<_>>();
    assert_eq!(numbered, [("custom:1:a", 1, 1), ("custom:1:b", 1, 2)]);
}

#[tokio::test(start_paused = true)]
async fn movie_details_carry_no_episodes() {
    let fake = Fake::default();
    let addons = [home_addon("meta.test")];
    fake.set(
        "meta.test",
        "meta",
        "tt1",
        Reply::json(json!({"meta": {
            "id":"tt1","type":"movie","name":"Aftersun","videos":[video("tt1", 1, 1)]
        }})),
    );
    let client = client(&fake).await;
    let events = client
        .details(&addons, Kind::Movie, "tt1", None)
        .collect::<Vec<_>>()
        .await;
    let ResourceEvent::Fresh(film) = &events[0] else {
        panic!("expected fresh details, got {events:?}");
    };
    assert_eq!(film.kind, Kind::Movie);
    assert!(film.episodes.is_empty());
}

#[tokio::test(start_paused = true)]
async fn stream_and_subtitle_requests_carry_the_kind_and_the_video_id_untouched() {
    let fake = Fake::default();
    let addons = [typed_addon(
        "streams.test",
        json!(["movie", "series"]),
        json!([]),
        json!([{"name":"stream","types":["series"],"idPrefixes":["tt"]},
            {"name":"subtitles","types":["series"],"idPrefixes":["tt"]}]),
    )];
    fake.set(
        "streams.test",
        "stream",
        "tt1:1:2",
        Reply::json(json!({"streams":[{"url":"https://cdn.test/e.mp4","name":"Episode"}]})),
    );
    fake.set(
        "streams.test",
        "subtitles",
        "tt1:1:2",
        Reply::json(json!({"subtitles":[{"id":"1","url":"https://subs.test/e.srt","lang":"eng"}]})),
    );
    let client = client(&fake).await;
    let events = client
        .streams(&addons, Kind::Series, "tt1:1:2")
        .collect::<Vec<_>>()
        .await;
    assert!(matches!(
        events.last(),
        Some(StreamsEvent::Groups(groups)) if groups.len() == 1
    ));
    let subtitles = client.subtitles(&addons, Kind::Series, "tt1:1:2").await;
    assert_eq!(subtitles.len(), 1);
    assert_eq!(subtitles[0].lang, "eng");
    assert_eq!(
        requests(&fake),
        [
            ("stream".into(), "series".into(), "tt1:1:2".into()),
            ("subtitles".into(), "series".into(), "tt1:1:2".into())
        ]
    );
    assert_eq!(
        client
            .streams(&addons, Kind::Movie, "tt1:1:2")
            .collect::<Vec<_>>()
            .await,
        [StreamsEvent::NoSources]
    );
}

#[tokio::test(start_paused = true)]
async fn kitsu_catalog_is_anime_while_its_items_and_episodes_are_series() {
    let fake = Fake::default();
    let addons = [kitsu()];
    fake.set(
        "kitsu.test",
        "catalog",
        "kitsu-anime-trending",
        Reply::json(json!({"metas":[show("kitsu:46474", "Frieren", "series")]})),
    );
    fake.set(
        "kitsu.test",
        "meta",
        "kitsu:46474",
        Reply::json(json!({"meta": {
            "id":"kitsu:46474","type":"series","name":"Frieren","releaseInfo":"2023–",
            "videos":[video("kitsu:46474:1", 1, 1), video("kitsu:46474:2", 1, 2)]
        }})),
    );
    fake.set(
        "kitsu.test",
        "stream",
        "kitsu:46474:1",
        Reply::json(json!({"streams":[{"url":"https://cdn.test/e.mp4"}]})),
    );
    let client = client(&fake).await;

    let catalogs = client.catalogs(&addons);
    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].kind, Kind::Anime);
    assert_eq!(catalogs[0].catalog_id, "kitsu-anime-trending");

    let events = client
        .catalog(&addons, Some(catalogs[0].clone()))
        .collect::<Vec<_>>()
        .await;
    let page = fresh_page(&events);
    assert_eq!(page.catalog.kind, Kind::Anime);
    assert_eq!(page.items[0].id, "kitsu:46474");
    assert_eq!(page.items[0].kind, Kind::Series);

    let events = client
        .details(&addons, page.items[0].kind, "kitsu:46474", None)
        .collect::<Vec<_>>()
        .await;
    let ResourceEvent::Fresh(film) = &events[0] else {
        panic!("expected fresh details, got {events:?}");
    };
    assert_eq!(film.kind, Kind::Series);
    assert_eq!(film.episodes[1].id, "kitsu:46474:2");
    assert_eq!(film.episodes[1].season, 1);
    assert_eq!(film.episodes[1].number, 2);

    client
        .streams(&addons, film.kind, &film.episodes[0].id)
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        requests(&fake),
        [
            (
                "catalog".into(),
                "anime".into(),
                "kitsu-anime-trending".into()
            ),
            ("meta".into(), "series".into(), "kitsu:46474".into()),
            ("stream".into(), "series".into(), "kitsu:46474:1".into()),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_series_top_and_a_movie_top_cache_apart() {
    let fake = Fake::default();
    let addons = [typed_addon(
        "both.test",
        json!(["movie", "series"]),
        json!([{"id":"top","type":"series","name":"Shows"},
            {"id":"top","type":"movie","name":"Films"}]),
        json!(["catalog"]),
    )];
    let client = client(&fake).await;
    let catalogs = client.catalogs(&addons);
    let kinds = catalogs
        .iter()
        .map(|catalog| catalog.kind)
        .collect::<Vec<_>>();
    assert_eq!(kinds, [Kind::Series, Kind::Movie]);

    fake.set(
        "both.test",
        "catalog",
        "top",
        Reply::json(json!({"metas":[show("tt-show", "A show", "series")]})),
    );
    client
        .catalog(&addons, Some(catalogs[0].clone()))
        .collect::<Vec<_>>()
        .await;
    fake.set("both.test", "catalog", "top", page("tt-film", "A film"));
    client
        .catalog(&addons, Some(catalogs[1].clone()))
        .collect::<Vec<_>>()
        .await;

    fake.set(
        "both.test",
        "catalog",
        "top",
        Reply::error(FailureKind::Offline),
    );
    for (selected, id) in [(&catalogs[0], "tt-show"), (&catalogs[1], "tt-film")] {
        let events = client
            .catalog(&addons, Some(selected.clone()))
            .collect::<Vec<_>>()
            .await;
        let ResourceEvent::CacheHit(cached) = &events[0] else {
            panic!("expected a cache hit first, got {events:?}");
        };
        assert_eq!(cached.items[0].id, id);
        assert_eq!(cached.catalog.kind, selected.kind);
    }

    let addon = client.addon_key(&addons[0]).unwrap();
    assert_eq!(
        Key::typed_catalog(addon.as_str(), "movie", "top").unwrap(),
        Key::catalog(addon.as_str(), "top").unwrap()
    );
    assert_ne!(
        Key::typed_catalog(addon.as_str(), "series", "top").unwrap(),
        Key::typed_catalog(addon.as_str(), "movie", "top").unwrap()
    );
}

#[tokio::test(start_paused = true)]
async fn home_defaults_to_a_movie_catalog_and_other_kinds_only_when_selected() {
    let fake = Fake::default();
    let addons = [typed_addon(
        "both.test",
        json!(["movie", "series"]),
        json!([{"id":"shows","type":"series","name":"Shows"},
            {"id":"films","type":"movie","name":"Films"}]),
        json!(["catalog"]),
    )];
    fake.set(
        "both.test",
        "catalog",
        "shows",
        Reply::json(json!({"metas":[show("tt-show", "A show", "series")]})),
    );
    fake.set("both.test", "catalog", "films", page("tt-film", "A film"));
    let client = client(&fake).await;

    let events = client.catalog(&addons, None).collect::<Vec<_>>().await;
    assert_eq!(fresh_page(&events).items[0].id, "tt-film");
    assert_eq!(fake.calls.lock().unwrap()[0].1.r#type, "movie");

    let shows = client.catalogs(&addons)[0].clone();
    assert_eq!(shows.kind, Kind::Series);
    let events = client
        .catalog(&addons, Some(shows))
        .collect::<Vec<_>>()
        .await;
    assert_eq!(fresh_page(&events).items[0].id, "tt-show");
}

#[test]
fn a_stored_catalog_selection_without_a_kind_is_a_movie_catalog() {
    let stored: CatalogRef = serde_json::from_value(json!({
        "addon": "both.test#0011223344556677", "catalog_id": "top", "name": "Films"
    }))
    .unwrap();
    assert_eq!(stored.kind, Kind::Movie);
}

#[tokio::test(start_paused = true)]
async fn search_asks_each_catalog_with_its_own_type_and_merges_kinds_in_account_order() {
    let fake = Fake::default();
    let search = json!([{"name":"search","isRequired":false}]);
    let addons = [
        typed_addon(
            "first.test",
            json!(["movie"]),
            json!([{"id":"films","type":"movie","name":"Films","extra":search}]),
            json!(["catalog"]),
        ),
        typed_addon(
            "kitsu.test",
            json!(["anime", "series"]),
            json!([{"id":"anime","type":"anime","name":"Anime","extra":search}]),
            json!(["catalog"]),
        ),
    ];
    fake.set("first.test", "catalog", "films", page("tt1", "Film"));
    fake.set(
        "kitsu.test",
        "catalog",
        "anime",
        Reply::json(json!({"metas":[show("kitsu:1", "Anime", "series")]})),
    );
    let client = client(&fake).await;
    let events = client.search(&addons, "title").collect::<Vec<_>>().await;
    let ResourceEvent::Fresh(films) = &events[0] else {
        panic!("expected results, got {events:?}");
    };
    let found = films
        .iter()
        .map(|film| (film.id.as_str(), film.kind))
        .collect::<Vec<_>>();
    assert_eq!(found, [("tt1", Kind::Movie), ("kitsu:1", Kind::Series)]);
    let calls = fake.calls.lock().unwrap();
    let types = calls
        .iter()
        .take(2)
        .map(|(host, path)| (host.as_str(), path.r#type.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(types, [("first.test", "movie"), ("kitsu.test", "anime")]);
}

#[test]
fn an_open_ended_series_year_counts_as_released_once_it_has_started() {
    let item = |release_info: &str| FilmDetails {
        id: release_info.into(),
        kind: Kind::Series,
        name: "Show".into(),
        original_name: None,
        release_info: Some(release_info.into()),
        released: None,
        runtime: None,
        genres: vec![],
        description: None,
        director: vec![],
        origin_country: None,
        cast: vec![],
        people: vec![],
        poster: None,
        background: None,
        logo: None,
        imdb_rating: None,
        links: vec![],
        episodes: vec![],
    };
    let mut films = vec![item("2026–"), item("2999–"), item("2015–2019")];
    only_released(&mut films);
    let kept = films
        .iter()
        .map(|film| film.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(kept, ["2026–", "2015–2019"]);
}

#[test]
fn a_film_and_a_show_with_one_id_cache_apart() {
    assert_eq!(
        Key::typed_meta("movie", "tmdb:1").unwrap(),
        Key::meta("tmdb:1").unwrap()
    );
    assert_ne!(
        Key::typed_meta("series", "tmdb:1").unwrap(),
        Key::meta("tmdb:1").unwrap()
    );
}
