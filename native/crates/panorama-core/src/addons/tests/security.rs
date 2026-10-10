use super::*;

#[tokio::test(start_paused = true)]
async fn spoofed_addon_id_cannot_poison_another_addons_cache() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let real = addon(
        "real.test",
        "same-id",
        json!([{"id":"top","type":"movie"}]),
        json!(["meta"]),
        Value::Null,
    );
    let spoof = addon(
        "evil.test",
        "same-id",
        json!([{"id":"top","type":"movie"}]),
        json!(["meta"]),
        Value::Null,
    );
    assert_ne!(
        client.addon_key(&real).unwrap(),
        client.addon_key(&spoof).unwrap()
    );
    fake.set("evil.test", "catalog", "top", page("tt1", "Poison"));
    fake.set(
        "evil.test",
        "meta",
        "tt1",
        Reply::json(json!({"meta":movie("tt1","Poison")})),
    );
    client
        .catalog(std::slice::from_ref(&spoof), None)
        .collect::<Vec<_>>()
        .await;
    client
        .details(&[spoof], "tt1", None)
        .collect::<Vec<_>>()
        .await;
    fake.set("real.test", "catalog", "top", page("tt1", "Real"));
    fake.set(
        "real.test",
        "meta",
        "tt1",
        Reply::json(json!({"meta":movie("tt1","Real")})),
    );
    let events = client
        .catalog(std::slice::from_ref(&real), None)
        .collect::<Vec<_>>()
        .await;
    assert_eq!(events.len(), 1);
    assert_eq!(fresh_page(&events).items[0].name, "Real");
    let events = client
        .details(&[real], "tt1", None)
        .collect::<Vec<_>>()
        .await;
    assert!(matches!(&events[..], [ResourceEvent::Fresh(details)] if details.name == "Real"));
}

#[tokio::test(start_paused = true)]
async fn cache_keys_and_errors_never_contain_transport_urls() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addon = home_addon("secret.test");
    let identity = client.addon_key(&addon).unwrap();
    let key = Key::catalog(identity.as_str(), "top").unwrap();
    assert!(key.as_str().starts_with("catalog:secret.test#"));
    assert_eq!(identity.as_str().split('#').nth(1).unwrap().len(), 16);
    for secret in [
        "https://",
        "secret-debrid-key",
        "token=secret",
        "manifest.json",
    ] {
        assert!(!key.as_str().contains(secret));
        assert!(!format!("{addon:?}").contains(secret));
    }
    fake.set(
        "secret.test",
        "catalog",
        "top",
        Reply::error(FailureKind::Network),
    );
    let events = client.catalog(&[addon], None).collect::<Vec<_>>().await;
    assert_eq!(events, [ResourceEvent::Failed(FailureKind::Network)]);
    assert!(!format!("{events:?}").contains("secret"));
    let invalid = home_addon("secret.test");
    let mut core = invalid.as_core().clone();
    core.manifest.id = "https://evil.test/token=secret".into();
    assert_eq!(
        client.addon_key(&Descriptor::from_core(core)),
        Err(FailureKind::InvalidInput)
    );
}

#[tokio::test(start_paused = true)]
async fn non_https_images_dropped_and_oversized_fields_truncated() {
    let fake = Fake::default();
    let client = client(&fake).await;
    fake.set("meta.test", "meta", "tt1", Reply::json(json!({"meta": {
        "id":"tt1","type":"movie","name":"é".repeat(400),"description":"界".repeat(6000),
        "releaseInfo":"1".repeat(150),"runtime":"1".repeat(150),
        "poster":"http://images.test/a","background":"file:///secret","logo":"data:image/png,abc",
        "links":[{"name":"bad","category":"site","url":"javascript:alert(1)"},
            {"name":"Director Name","category":"Director","url":"https://people.test/director"},
            {"name":"Actor","category":"Cast","url":"https://people.test/actor"},
            {"name":"Drama","category":"Genres","url":"stremio:///discover"},
            {"name":"8.1","category":"imdb","url":"https://imdb.com/title/tt1"}]
    }})));
    let events = client
        .details(&[home_addon("meta.test")], "tt1", None)
        .collect::<Vec<_>>()
        .await;
    let ResourceEvent::Fresh(film) = &events[0] else {
        panic!("expected fresh");
    };
    assert_eq!(film.name.chars().count(), 300);
    assert_eq!(film.description.as_ref().unwrap().chars().count(), 5000);
    assert_eq!(film.release_info.as_ref().unwrap().chars().count(), 100);
    assert_eq!(film.runtime.as_ref().unwrap().chars().count(), 100);
    assert!(film.poster.is_none() && film.background.is_none() && film.logo.is_none());
    assert_eq!(film.director, ["Director Name"]);
    assert_eq!(film.cast, ["Actor"]);
    assert_eq!(film.genres, ["Drama"]);
    assert_eq!(film.imdb_rating.as_deref(), Some("8.1"));
    assert_eq!(film.links.len(), 3);
    assert!(
        film.links
            .iter()
            .all(|link| matches!(link.url.scheme(), "http" | "https"))
    );
}

#[tokio::test(start_paused = true)]
async fn film_id_validation_rejects_path_injection() {
    let fake = Fake::default();
    let client = client(&fake).await;
    for id in [
        "",
        "../secret",
        "tt1?token=secret",
        "tt1#fragment",
        "a b",
        "a\n",
        "a\0",
        "a\u{2003}",
        &"a".repeat(201),
    ] {
        assert_eq!(
            client.details(&[], id, None).collect::<Vec<_>>().await,
            [ResourceEvent::Failed(FailureKind::InvalidInput)]
        );
        assert_eq!(
            client
                .streams(&[home_addon("test.test")], id)
                .collect::<Vec<_>>()
                .await,
            [StreamsEvent::Failed(FailureKind::InvalidInput)]
        );
    }
    assert!(fake.hosts().is_empty());
    assert!(sanitize::valid_id("opaque:雪%2F"));
}

#[tokio::test(start_paused = true)]
async fn responses_over_eight_mib_rejected_and_lists_capped() {
    let oversized = TransportResponse::Bytes(vec![b' '; sanitize::BODY_LIMIT + 1]);
    assert!(matches!(
        sanitize::parse(oversized),
        Err(FailureKind::TooLarge)
    ));
    let fake = Fake::default();
    let client = client(&fake).await;
    fake.set(
        "large.test",
        "meta",
        "custom:large",
        Reply {
            delay: Duration::ZERO,
            value: Ok(vec![b' '; sanitize::BODY_LIMIT + 1]),
        },
    );
    assert_eq!(
        client
            .details(&[home_addon("large.test")], "custom:large", None)
            .collect::<Vec<_>>()
            .await,
        [ResourceEvent::Failed(FailureKind::TooLarge)]
    );
    let metas = (0..250)
        .map(|index| movie(&format!("tt{index}"), "Film"))
        .collect::<Vec<_>>();
    fake.set(
        "cap.test",
        "catalog",
        "top",
        Reply::json(json!({"metas": metas})),
    );
    let events = client
        .catalog(&[home_addon("cap.test")], None)
        .collect::<Vec<_>>()
        .await;
    assert_eq!(fresh_page(&events).items.len(), 200);
    fake.set("cap.test", "stream", "tt1", Reply::json(json!({"streams":(0..250).map(|_| json!({"url":"https://video.test/a","name":"n".repeat(400),"title":"d".repeat(6000)})).collect::<Vec<_>>()})));
    let events = client
        .streams(&[home_addon("cap.test")], "tt1")
        .collect::<Vec<_>>()
        .await;
    let StreamsEvent::Groups(groups) = &events[0] else {
        panic!("expected groups");
    };
    let StreamState::Ready(sources) = &groups[0].state else {
        panic!("expected sources");
    };
    assert_eq!(sources.len(), 200);
    assert_eq!(sources[0].name.as_ref().unwrap().len(), 300);
    assert_eq!(sources[0].description.as_ref().unwrap().len(), 5000);
}

#[tokio::test(start_paused = true)]
async fn unicode_film_ids_and_installation_key_characters_are_supported() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [home_addon("unicode.test")];
    let id = "😀".repeat(200);
    let key = Key::meta(&id).unwrap();
    assert_eq!(key.as_str(), format!("meta:{id}"));
    let identity = client.addon_key(&addons[0]).unwrap();
    assert!(Key::catalog(identity.as_str(), "top").is_ok());
    fake.set(
        "unicode.test",
        "meta",
        &id,
        Reply::json(json!({"meta":movie(&id,"Unicode")})),
    );
    let events = client.details(&addons, &id, None).collect::<Vec<_>>().await;
    assert!(matches!(&events[..], [ResourceEvent::Fresh(film)] if film.id == id));
    assert!(client.store.get(&key).unwrap().is_some());
}

#[tokio::test(start_paused = true)]
async fn installation_salt_is_atomic_persistent_and_unique_to_the_store() {
    let fake = Fake::default();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (first, second) = tokio::join!(
        AddonClient::new(store.clone(), fake.clone()),
        AddonClient::new(store, fake.clone())
    );
    let first = first.unwrap();
    let second = second.unwrap();
    let addon = home_addon("installed.test");
    assert_eq!(
        first.addon_key(&addon).unwrap(),
        second.addon_key(&addon).unwrap()
    );
    let different = client(&fake).await;
    assert_ne!(
        first.addon_key(&addon).unwrap(),
        different.addon_key(&addon).unwrap()
    );
}

#[tokio::test(start_paused = true)]
async fn oversized_transport_urls_fail_without_panicking_or_exposing_secrets() {
    for path in ["secret".repeat(12_000), "manifest.json".repeat(100)] {
        let base = format!("https://addon.test/{path}/manifest.json?token=secret")
            .parse()
            .unwrap();
        let request = ResourceRequest::new(
            base,
            ResourcePath::without_extra("meta", "movie", &"é".repeat(200)),
        );
        let result = CoreAddonTransport.resource(request).await;
        assert!(matches!(result, Err(FailureKind::InvalidInput)));
    }
}

#[tokio::test(start_paused = true)]
async fn sign_out_clears_addon_caches_but_retains_random_installation_salt() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [home_addon("installed.test")];
    fake.set("installed.test", "catalog", "top", page("tt1", "Film"));
    fake.set(
        "installed.test",
        "meta",
        "tt1",
        Reply::json(json!({"meta":movie("tt1","Film")})),
    );
    client.catalog(&addons, None).collect::<Vec<_>>().await;
    client
        .details(&addons, "tt1", None)
        .collect::<Vec<_>>()
        .await;
    let identity = client.addon_key(&addons[0]).unwrap();
    let salt_key = Key::pref("installSalt").unwrap();
    let salt = client.store.get(&salt_key).unwrap().unwrap();
    assert_eq!(salt.len(), 32);
    let catalog_key = Key::catalog(identity.as_str(), "top").unwrap();
    assert!(client.store.get(&catalog_key).unwrap().is_some());
    assert!(
        client
            .store
            .get(&Key::meta("tt1").unwrap())
            .unwrap()
            .is_some()
    );
    let store = client.store.clone();
    tokio::task::spawn_blocking(move || store.clear_on_sign_out())
        .await
        .unwrap()
        .unwrap();
    assert!(client.store.get(&catalog_key).unwrap().is_none());
    assert!(
        client
            .store
            .get(&Key::meta("tt1").unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(client.store.get(&salt_key).unwrap().unwrap(), salt);
    let restarted = AddonClient::new(client.store.clone(), fake).await.unwrap();
    assert_eq!(restarted.addon_key(&addons[0]).unwrap(), identity);
    assert!(!String::from_utf8_lossy(&salt).contains("installed.test"));
}
