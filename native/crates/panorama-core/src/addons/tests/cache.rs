use super::*;

#[derive(Clone)]
struct QueuedWrite {
    response: Vec<u8>,
    release: Arc<Mutex<Option<std::sync::mpsc::Receiver<()>>>>,
    queued: Arc<tokio::sync::Notify>,
}

impl AddonTransport for QueuedWrite {
    fn resource(
        &self,
        _: ResourceRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, FailureKind>> {
        let transport = self.clone();
        async move {
            let release = transport.release.lock().unwrap().take().unwrap();
            let (started, occupied) = tokio::sync::oneshot::channel();
            tokio::task::spawn_blocking(move || {
                started.send(()).unwrap();
                release.recv().unwrap();
            });
            occupied.await.unwrap();
            transport.queued.notify_one();
            Ok(TransportResponse::Bytes(transport.response))
        }
        .boxed()
    }
}

fn cancelled_write_cannot_restore_cache_after_sign_out(metadata: bool) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (release, receiver) = std::sync::mpsc::channel();
        let queued = Arc::new(tokio::sync::Notify::new());
        let film = movie("custom:1", "Film");
        let response = if metadata {
            json!({"meta":film})
        } else {
            json!({"metas":[film]})
        };
        let transport = QueuedWrite {
            response: serde_json::to_vec(&response).unwrap(),
            release: Arc::new(Mutex::new(Some(receiver))),
            queued: queued.clone(),
        };
        let store = Arc::new(Store::open_in_memory().unwrap());
        let client = AddonClient::new(store.clone(), transport).await.unwrap();
        let addons = [home_addon("installed.test")];
        let key = if metadata {
            Key::meta("custom:1").unwrap()
        } else {
            Key::catalog(client.addon_key(&addons[0]).unwrap().as_str(), "top").unwrap()
        };
        let stream = if metadata {
            client
                .details(&addons, "custom:1", None)
                .map(|_| ())
                .boxed()
        } else {
            client.catalog(&addons, None).map(|_| ()).boxed()
        };
        queued.notified().await;
        drop(stream);
        tokio::task::yield_now().await;
        store.clear_on_sign_out().unwrap();
        release.send(()).unwrap();
        tokio::task::spawn_blocking(|| {}).await.unwrap();
        assert!(store.get(&key).unwrap().is_none());
    });
}

#[test]
fn cancelled_catalog_write_cannot_restore_cache_after_sign_out() {
    cancelled_write_cannot_restore_cache_after_sign_out(false);
}

#[test]
fn cancelled_details_write_cannot_restore_cache_after_sign_out() {
    cancelled_write_cannot_restore_cache_after_sign_out(true);
}

#[tokio::test(start_paused = true)]
async fn sign_out_discards_inflight_cache_writes_and_allows_new_requests() {
    for metadata in [false, true] {
        let fake = Fake::default();
        let client = client(&fake).await;
        let addons = [home_addon("installed.test")];
        let (resource, id, reply, key) = if metadata {
            (
                "meta",
                "custom:1",
                Reply::json(json!({"meta":movie("custom:1", "Film")})),
                Key::meta("custom:1").unwrap(),
            )
        } else {
            (
                "catalog",
                "top",
                page("custom:1", "Film"),
                Key::catalog(client.addon_key(&addons[0]).unwrap().as_str(), "top").unwrap(),
            )
        };
        fake.set("installed.test", resource, id, reply.delayed(1));
        let stream = if metadata {
            client
                .details(&addons, "custom:1", None)
                .map(|_| ())
                .boxed()
        } else {
            client.catalog(&addons, None).map(|_| ()).boxed()
        };
        while fake.hosts().is_empty() {
            tokio::task::yield_now().await;
        }
        client.store.clear_on_sign_out().unwrap();
        stream.collect::<Vec<_>>().await;
        assert!(client.store.get(&key).unwrap().is_none());
        if metadata {
            client
                .details(&addons, "custom:1", None)
                .collect::<Vec<_>>()
                .await;
        } else {
            client.catalog(&addons, None).collect::<Vec<_>>().await;
        }
        assert!(client.store.get(&key).unwrap().is_some());
    }
}

#[tokio::test(start_paused = true)]
async fn search_enrichment_keeps_the_original_sign_out_generation() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let addons = [addon(
        "installed.test",
        "installed",
        json!([
            {"id":"find","type":"movie","extra":[{"name":"search"}]}
        ]),
        json!(["meta"]),
        Value::Null,
    )];
    fake.set(
        "installed.test",
        "catalog",
        "find",
        Reply::json(json!({"metas":[
            {"id":"custom:1","type":"movie","name":"Film"}
        ]})),
    );
    fake.set(
        "installed.test",
        "meta",
        "custom:1",
        Reply::json(json!({"meta":movie("custom:1", "Film")})),
    );
    let stream = client.search(&addons, "film");
    client.store.clear_on_sign_out().unwrap();
    let events = stream.collect::<Vec<_>>().await;
    assert!(
        matches!(events.last(), Some(ResourceEvent::Fresh(items)) if items[0].poster.is_some())
    );
    assert!(
        client
            .store
            .get(&Key::meta("custom:1").unwrap())
            .unwrap()
            .is_none()
    );
}

#[tokio::test(start_paused = true)]
async fn home_cache_read_failure_still_fetches_addons_and_cinemeta() {
    for fallback in [false, true] {
        let fake = Fake::default();
        let client = client(&fake).await;
        let addons = [home_addon("installed.test")];
        let key = Key::catalog(client.addon_key(&addons[0]).unwrap().as_str(), "top").unwrap();
        client.store.inject_read_failure(&key);
        assert!(client.store.get(&key).is_err());
        let host = if fallback {
            "v3-cinemeta.strem.io"
        } else {
            "installed.test"
        };
        fake.set(host, "catalog", "top", page("tt1", "Fresh"));
        let events = client.catalog(&addons, None).collect::<Vec<_>>().await;
        assert_eq!(fresh_page(&events).items[0].name, "Fresh");
        assert_eq!(fake.hosts().last().unwrap(), host);
    }
}

#[tokio::test(start_paused = true)]
async fn details_cache_read_failure_still_fetches_addons_and_cinemeta() {
    for fallback in [false, true] {
        let fake = Fake::default();
        let client = client(&fake).await;
        let addons = [home_addon("installed.test")];
        let key = Key::meta("tt1").unwrap();
        client.store.inject_read_failure(&key);
        assert!(client.store.get(&key).is_err());
        let host = if fallback {
            "v3-cinemeta.strem.io"
        } else {
            "installed.test"
        };
        fake.set(
            host,
            "meta",
            "tt1",
            Reply::json(json!({"meta":movie("tt1", "Fresh")})),
        );
        let events = client
            .details(&addons, "tt1", None)
            .collect::<Vec<_>>()
            .await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ResourceEvent::Fresh(film) if film.name == "Fresh"))
        );
        assert_eq!(fake.hosts().last().unwrap(), host);
        assert!(client.store.get(&key).unwrap().is_some());
    }
}
