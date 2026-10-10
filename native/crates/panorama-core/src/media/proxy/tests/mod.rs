//! Test equivalents from `tests/unit/media-proxy.test.ts`.
mod failures;
mod fixture;
mod integrity;
mod regression;
mod upstream;
use super::*;
use bytes::Bytes;
use fixture::*;
use std::sync::atomic::Ordering;

#[tokio::test(start_paused = true)]
async fn serves_exact_byte_ranges_assembled_from_parallel_upstream_requests() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    let whole = drive(async {
        reqwest::get(&handle.url)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
    })
    .await;
    assert_eq!(whole, fixture.0.bytes);
    let middle = drive(
        reqwest::Client::new()
            .get(&handle.url)
            .header("Range", "bytes=4194000-6300000")
            .send(),
    )
    .await
    .unwrap();
    assert_eq!(middle.status(), 206);
    assert_eq!(
        middle.headers()["content-range"],
        format!("bytes 4194000-6300000/{}", fixture.0.bytes.len())
    );
    assert_eq!(
        drive(middle.bytes()).await.unwrap(),
        fixture.0.bytes.slice(4_194_000..6_300_001)
    );
    assert!(
        fixture
            .ranges()
            .iter()
            .filter(|range| !range.starts_with("bytes=0-"))
            .count()
            > 3
    );
    assert_eq!(
        handle.stats().size_bytes,
        Some(fixture.0.bytes.len() as u64)
    );
    assert!(!handle.stats().transfer_demanded);
    assert_eq!(handle.stats().download_mbps, None);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn abandons_silently_stalled_upstream_requests_and_still_delivers_every_byte() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).stalls = 3;
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    assert_eq!(
        drive(read(&handle, 0, fixture.0.bytes.len() as u64 - 1))
            .await
            .unwrap(),
        fixture.0.bytes
    );
    assert_eq!(fixture.0.held.load(Ordering::SeqCst), 3);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn recovers_a_throttled_probe_within_the_same_playback_request() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).probe_throttle = true;
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorder = events.clone();
    proxy.set_event_sink(Some(Arc::new(move |event| lock(&recorder).push(event))));
    let response = drive(reqwest::get(&handle.url)).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(drive(response.bytes()).await.unwrap(), fixture.0.bytes);
    assert_eq!(fixture.0.opening.load(Ordering::SeqCst), 2);
    assert!(lock(&events).iter().any(|event| matches!(
        event,
        MediaProxyEvent::Probe {
            result: ProbeResult::Ranged,
            ..
        }
    )));
    assert_eq!(
        lock(&handle.session.state).mode,
        Some(session::Mode::Ranged)
    );
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn refreshes_an_expired_redirect_target_once_instead_of_per_range() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).expire = true;
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    assert_eq!(
        drive(read(&handle, 0, fixture.0.bytes.len() as u64 - 1))
            .await
            .unwrap(),
        fixture.0.bytes
    );
    assert_eq!(fixture.0.resolver_hits.load(Ordering::SeqCst), 2);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn uses_one_connection_when_the_player_has_plenty_buffered_and_several_when_it_is_short() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).delay = true;
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    handle.set_read_ahead(sample(300.0, false));
    assert_eq!(
        drive(read(&handle, 0, fixture.0.bytes.len() as u64 - 1))
            .await
            .unwrap(),
        fixture.0.bytes
    );
    assert_eq!(fixture.0.peak.load(Ordering::SeqCst), 1);
    fixture.0.peak.store(0, Ordering::SeqCst);
    let handle = drive(proxy.create_session(Fixture::source()))
        .await
        .unwrap();
    handle.set_read_ahead(sample(2.0, true));
    assert_eq!(
        drive(read(&handle, 0, fixture.0.bytes.len() as u64 - 1))
            .await
            .unwrap(),
        fixture.0.bytes
    );
    assert!(fixture.0.peak.load(Ordering::SeqCst) > 2);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn streams_the_first_bytes_before_a_range_completes_and_resumes_a_dropped_range() {
    let fixture = Fixture::new();
    {
        let mut faults = lock(&fixture.0.faults);
        faults.slow = true;
        faults.drop_range = true;
    }
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    let start = tokio::time::Instant::now();
    drive(handle.session.prepare()).await.unwrap();
    let mut reader = handle.session.add_reader(0).unwrap();
    let end = fixture.0.bytes.len() as u64 - 1;
    let first = drive(reader.read(end)).await.unwrap().unwrap();
    assert!(!first.is_empty());
    assert!(start.elapsed() < Duration::from_millis(300));
    let mut bytes = first.to_vec();
    while let Some(part) = drive(reader.read(end)).await.unwrap() {
        bytes.extend_from_slice(&part);
    }
    assert_eq!(Bytes::from(bytes), fixture.0.bytes);
    assert!(fixture.ranges().contains(&format!(
        "bytes={}-{}",
        CHUNK_BYTES + 1_000_000,
        2 * CHUNK_BYTES - 1
    )));
    drop(reader);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn warms_the_opening_and_tail_and_reuses_both_when_playback_opens_the_same_source() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, old) = drive(setup(&fixture, false)).await;
    drive(old.close()).await;
    assert_eq!(
        drive(proxy.warm(Fixture::source())).await.unwrap(),
        MediaWarmResult {
            ready: true,
            error: None,
            unreachable: false
        }
    );
    until(|| fixture.ranges().len() >= 4).await;
    let mut warmed = fixture.ranges();
    warmed.sort();
    assert_eq!(
        warmed,
        [
            "bytes=0-2097151",
            "bytes=2097152-4194303",
            "bytes=4194304-6291455",
            &format!("bytes=8388608-{}", fixture.0.bytes.len() - 1)
        ]
    );
    let handle = drive(proxy.create_session(Fixture::source()))
        .await
        .unwrap();
    assert_eq!(
        drive(read(&handle, 0, 6_291_455)).await.unwrap(),
        fixture.0.bytes.slice(..6_291_456)
    );
    assert_eq!(
        drive(read(&handle, 8_388_608, fixture.0.bytes.len() as u64 - 1))
            .await
            .unwrap(),
        fixture.0.bytes.slice(8_388_608..)
    );
    for range in warmed {
        assert_eq!(
            fixture
                .ranges()
                .iter()
                .filter(|value| **value == range)
                .count(),
            1
        );
    }
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn cancels_an_unfinished_tail_warm_up_without_retrying_it() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).pending_tail = true;
    let (_dir, mut proxy, _) = drive(setup(&fixture, false)).await;
    assert!(drive(proxy.warm(Fixture::source())).await.unwrap().ready);
    until(|| {
        fixture
            .ranges()
            .iter()
            .any(|range| range.starts_with("bytes=8388608-"))
    })
    .await;
    drive(proxy.close()).await;
    assert_eq!(fixture.0.open.load(Ordering::SeqCst), 0);
    tokio::time::advance(Duration::from_millis(50)).await;
    assert_eq!(
        fixture
            .ranges()
            .iter()
            .filter(|range| range.starts_with("bytes=8388608-"))
            .count(),
        1
    );
}
#[tokio::test(start_paused = true)]
async fn cancels_a_queued_tail_warm_up_within_the_connection_cap() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).pending_all = true;
    let (_dir, mut proxy, _) = drive(setup(&fixture, true)).await;
    assert!(drive(proxy.warm(Fixture::source())).await.unwrap().ready);
    until(|| fixture.ranges().len() == 3).await;
    assert!(
        !fixture
            .ranges()
            .iter()
            .any(|range| range.starts_with("bytes=8388608-"))
    );
    drive(proxy.close()).await;
    tokio::time::advance(Duration::from_millis(50)).await;
    assert_eq!(fixture.ranges().len(), 3);
}
#[tokio::test(start_paused = true)]
async fn reuses_an_in_progress_range_across_a_brief_gap_between_player_reads() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).slow = true;
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    assert_eq!(
        drive(read(&handle, 0, 65535)).await.unwrap(),
        fixture.0.bytes.slice(..65536)
    );
    tokio::time::advance(Duration::from_millis(25)).await;
    assert_eq!(
        drive(read(&handle, 65536, 131071)).await.unwrap(),
        fixture.0.bytes.slice(65536..131072)
    );
    assert_eq!(fixture.0.opening.load(Ordering::SeqCst), 1);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn remembers_a_source_whose_server_refuses_connections_and_fails_fast() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).failed = true;
    let (_dir, mut proxy, _handle) = drive(setup(&fixture, false)).await;
    let result = drive(proxy.warm(Fixture::source())).await.unwrap();
    assert!(!result.ready);
    assert!(result.unreachable);
    assert!(result.error.is_some());
    let started = tokio::time::Instant::now();
    assert!(
        drive(proxy.warm(Fixture::source()))
            .await
            .unwrap()
            .unreachable
    );
    let handle = drive(proxy.create_session(Fixture::source()))
        .await
        .unwrap();
    assert_eq!(
        drive(reqwest::get(&handle.url)).await.unwrap().status(),
        502
    );
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(handle.stats().unreachable);
    assert!(handle.stats().error.is_some());
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn passes_through_sources_that_do_not_support_range_requests() {
    let fixture = Fixture::new();
    lock(&fixture.0.faults).ranged = false;
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    let response = drive(reqwest::get(&handle.url)).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(drive(response.bytes()).await.unwrap(), fixture.0.bytes);
    assert_eq!(handle.stats().size_bytes, None);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn rejects_unknown_and_replaced_sessions() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, first) = drive(setup(&fixture, false)).await;
    drive(proxy.create_session(Fixture::source()))
        .await
        .unwrap();
    assert_eq!(drive(reqwest::get(&first.url)).await.unwrap().status(), 404);
    let unknown = format!(
        "{}/{}",
        first.url.rsplit_once('/').unwrap().0,
        "0".repeat(32)
    );
    assert_eq!(drive(reqwest::get(unknown)).await.unwrap().status(), 404);
    drive(proxy.close()).await;
}
