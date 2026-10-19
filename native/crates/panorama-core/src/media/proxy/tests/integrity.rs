//! Additional representation, resolver, security and lifecycle tests for `media-proxy.ts`.
use super::*;
use crate::media::fetch::MediaResolver;
use futures::future::BoxFuture;

async fn with_resolver(
    fixture: &Fixture,
    fails: bool,
    different_size: bool,
) -> (
    tempfile::TempDir,
    MediaProxy,
    SessionHandle,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    lock(&fixture.0.faults).expire = true;
    lock(&fixture.0.faults).changed_size = different_size;
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = hits.clone();
    let resolver: Arc<dyn MediaResolver> = Arc::new(
        move |_: MediaSource| -> BoxFuture<'static, Result<MediaSource, MediaError>> {
            count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if fails {
                    Err(MediaError::Transport)
                } else {
                    MediaSource::new("http://fixture.invalid/new?secret=fresh")
                }
            })
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let mut options = ProxyOptions::default();
    options.cache.directory = dir.path().to_owned();
    options.cache.reserve_free_bytes = 0;
    let mut proxy = MediaProxy::with_fetch(options, Arc::new(fixture.clone()), Some(resolver))
        .await
        .unwrap();
    let handle = proxy.create_session(Fixture::source()).await.unwrap();
    (dir, proxy, handle, hits)
}
#[tokio::test(start_paused = true)]
async fn link_expires_mid_stream_playback_bytes_continue_and_are_byte_identical() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, handle, hits) = drive(with_resolver(&fixture, false, false)).await;
    assert_eq!(
        drive(read(&handle, 0, fixture.0.bytes.len() as u64 - 1))
            .await
            .unwrap(),
        fixture.0.bytes
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(handle.stats().error.is_none());
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn resolver_fails_session_reports_error_and_stops_retrying() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, handle, hits) = drive(with_resolver(&fixture, true, false)).await;
    assert!(
        drive(read(&handle, 0, fixture.0.bytes.len() as u64 - 1))
            .await
            .is_err()
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(handle.stats().error, Some(MediaError::ResolverFailed));
    let count = fixture.ranges().len();
    tokio::time::advance(Duration::from_secs(600)).await;
    assert_eq!(fixture.ranges().len(), count);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn resolved_file_has_a_different_size_rejected() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, handle, hits) = drive(with_resolver(&fixture, false, true)).await;
    assert!(
        drive(read(&handle, 0, fixture.0.bytes.len() as u64 - 1))
            .await
            .is_err()
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        handle.stats().error,
        Some(MediaError::Representation("Resolved media size changed"))
    );
    assert!(handle.stats().representation_changed);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn refresh_budget_and_backoff_are_per_session_and_rolling() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, handle, hits) = drive(with_resolver(&fixture, false, false)).await;
    drive(handle.session.prepare()).await.unwrap();
    drive(handle.session.reresolve(0)).await.unwrap();
    for generation in 1..3 {
        let deadline =
            *lock(&handle.session.state).resolves.back().unwrap() + Duration::from_secs(30);
        let refresh = handle.session.reresolve(generation);
        tokio::pin!(refresh);
        for _ in 0..3 {
            assert!(futures::poll!(&mut refresh).is_pending());
            assert_eq!(hits.load(Ordering::SeqCst), generation as usize);
        }
        tokio::time::advance(deadline - tokio::time::Instant::now() - Duration::from_millis(1))
            .await;
        assert!(futures::poll!(&mut refresh).is_pending());
        assert_eq!(hits.load(Ordering::SeqCst), generation as usize);
        tokio::time::advance(Duration::from_millis(1)).await;
        drive(refresh).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), generation as usize + 1);
    }
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert!(
        tokio::time::Instant::now().duration_since(lock(&handle.session.state).resolves[0])
            >= Duration::from_secs(60)
    );
    assert_eq!(
        drive(handle.session.reresolve(3)).await,
        Err(MediaError::ResolveLimit)
    );
    tokio::time::advance(Duration::from_secs(600)).await;
    drive(handle.session.reresolve(3)).await.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 4);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn rejects_wrong_host_methods_query_tokens_and_explicitly_closed_sessions() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    let client = reqwest::Client::new();
    for request in [
        client.get(&handle.url).header("host", "attacker.invalid"),
        client.post(&handle.url),
        client.get(format!("{}?token=extra", handle.url)),
    ] {
        assert_eq!(drive(request.send()).await.unwrap().status(), 404);
    }
    assert_eq!(
        drive(client.head(&handle.url).send())
            .await
            .unwrap()
            .status(),
        200
    );
    drive(handle.close()).await;
    assert_eq!(
        drive(client.get(&handle.url).send())
            .await
            .unwrap()
            .status(),
        404
    );
    drive(proxy.close()).await;
}
#[test]
fn client_range_contract_includes_suffix_invalid_and_unsatisfiable_ranges() {
    assert_eq!(server::client_range(None, 10), Ok((0, 9, false)));
    assert_eq!(server::client_range(Some("garbage"), 10), Ok((0, 9, false)));
    assert_eq!(server::client_range(Some("bytes=-3"), 10), Ok((7, 9, true)));
    assert_eq!(
        server::client_range(Some("bytes=2-99"), 10),
        Ok((2, 9, true))
    );
    assert_eq!(server::client_range(Some("bytes=-"), 10), Ok((0, 9, true)));
    for value in [
        "bytes=-0",
        "bytes=10-",
        "bytes=5-2",
        "bytes=9007199254740992-",
    ] {
        assert!(server::client_range(Some(value), 10).is_err());
    }
}
#[tokio::test(start_paused = true)]
async fn client_that_stops_reading_mid_body_cannot_make_close_hang() {
    let fixture = Fixture::new();
    let (dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    let response = drive(reqwest::get(&handle.url)).await.unwrap();
    let started = tokio::time::Instant::now();
    drive(proxy.close()).await;
    assert!(started.elapsed() <= Duration::from_secs(1));
    assert_eq!(proxy.cache_stats(), CacheStats::default());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert!(drive(reqwest::get(&handle.url)).await.is_err());
    drop(response);
}
#[tokio::test(start_paused = true)]
async fn dropping_proxy_cancels_listener_connections_and_pending_transfers() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).pending_all = true;
    let (dir, proxy, handle) = drive(setup(&fixture, false)).await;
    let response = drive(reqwest::get(&handle.url)).await.unwrap();
    drop(proxy);
    until(|| fixture.0.open.load(Ordering::SeqCst) == 0).await;
    assert!(drive(reqwest::get(&handle.url)).await.is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    drop(response);
}
#[tokio::test(start_paused = true)]
async fn warm_up_expires_and_reservations_do_not_survive_close() {
    let fixture = Fixture::new();
    let (dir, mut proxy, _) = drive(setup(&fixture, false)).await;
    assert!(drive(proxy.warm(Fixture::source())).await.unwrap().ready);
    tokio::time::advance(Duration::from_secs(600)).await;
    drive(async {
        if let Some(warm) = &proxy.warmed {
            until(|| warm.session.stopped()).await;
        }
    })
    .await;
    drive(proxy.close()).await;
    assert_eq!(proxy.cache_stats(), CacheStats::default());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
#[test]
fn source_session_errors_and_cache_names_redact_secret_urls() {
    let source = Fixture::source();
    assert!(!format!("{source:?}").contains("secret"));
    for value in [
        "ftp://host/file",
        "http://user:pass@host/file",
        "data:text/plain,secret",
        "not a URL",
    ] {
        let error = MediaSource::new(value).unwrap_err();
        assert!(!error.to_string().contains(value));
        assert!(!format!("{error:?}").contains(value));
    }
}

#[tokio::test(start_paused = true)]
async fn http_date_retry_deadlines_follow_virtual_time_without_real_sleep() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    let now = handle.session.unix_epoch_ms;
    let date = httpdate::fmt_http_date(std::time::UNIX_EPOCH + Duration::from_millis(now + 60_000));
    handle.session.throttle(Some(&date));
    let first = lock(&handle.session.state).paused_until;
    tokio::time::advance(Duration::from_secs(30)).await;
    handle.session.throttle(Some(&date));
    assert_eq!(lock(&handle.session.state).paused_until, first);
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(handle.session.cooldown().await.is_ok());
    drive(proxy.close()).await;
}

#[tokio::test(start_paused = true)]
async fn urgent_seek_preempts_unneeded_prefetch_within_connection_cap() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).pending_all = true;
    let (_dir, mut proxy, handle) = drive(setup(&fixture, true)).await;
    drive(handle.session.prepare()).await.unwrap();
    let mut opening = handle.session.add_reader(0).unwrap();
    assert_eq!(
        drive(opening.read(CHUNK_BYTES - 1))
            .await
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    until(|| fixture.ranges().len() == 3).await;
    let mut seek = handle.session.add_reader(3 * CHUNK_BYTES).unwrap();
    assert_eq!(
        drive(seek.read(3 * CHUNK_BYTES)).await.unwrap().unwrap(),
        fixture
            .0
            .bytes
            .slice(3 * CHUNK_BYTES as usize..3 * CHUNK_BYTES as usize + 1)
    );
    assert!(fixture.0.peak.load(Ordering::SeqCst) <= 3);
    assert_eq!(handle.stats().parallel, 3);
    drop(seek);
    drop(opening);
    drive(proxy.close()).await;
}

#[tokio::test(start_paused = true)]
async fn memory_only_single_chunk_budget_releases_consumed_uncacheable_chunks() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-directory");
    std::fs::write(&file, b"fixture").unwrap();
    let mut options = ProxyOptions::default();
    options.cache.directory = file;
    options.cache.max_memory_bytes = CHUNK_BYTES;
    options.cache.reserve_free_bytes = 0;
    let mut proxy = drive(MediaProxy::with_fetch(
        options,
        Arc::new(fixture.clone()),
        None,
    ))
    .await
    .unwrap();
    let handle = drive(proxy.create_session(Fixture::source()))
        .await
        .unwrap();
    handle.set_read_ahead(sample(300.0, false));
    assert_eq!(
        drive(read(&handle, 0, 2 * CHUNK_BYTES - 1)).await.unwrap(),
        fixture.0.bytes.slice(..2 * CHUNK_BYTES as usize)
    );
    let stats = handle.stats();
    assert!(stats.cache.memory_bytes + stats.cache.reserved_bytes <= CHUNK_BYTES);
    assert_eq!(stats.cache.disk_bytes, 0);
    drive(proxy.close()).await;
}

#[tokio::test(start_paused = true)]
async fn session_debug_and_disk_names_never_contain_source_urls() {
    let fixture = Fixture::new();
    let (dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    assert!(!format!("{handle:?} {:?}", handle.session).contains("fixture.invalid"));
    drive(read(&handle, 0, CHUNK_BYTES - 1)).await.unwrap();
    until(|| proxy.cache_stats().disk_bytes > 4096).await;
    for owner in std::fs::read_dir(dir.path()).unwrap() {
        for file in std::fs::read_dir(owner.unwrap().path()).unwrap() {
            let name = file.unwrap().file_name().to_string_lossy().into_owned();
            assert!(
                name == "owner.lock"
                    || (name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit()))
            );
        }
    }
    drive(proxy.close()).await;
}
