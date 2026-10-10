use super::super::*;
use super::fixture::{Swarm, connect, get};
use tokio::{
    io::AsyncReadExt,
    time::{Instant, timeout},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stream_queue_lookahead_and_no_reader_pause() {
    let swarm = Swarm::new().await;
    let (source, data) = swarm.video("window.mp4", 64 * 1024 * 1024).await;
    let engine = swarm.engine(swarm.dir.path().join("cache"), 64 * 1024 * 1024);
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    let host = engine.host_for_test().await;
    let entry = stream.playback.entry.clone();
    let index = stream.playback.index;
    let (reader, first) = host
        .handle
        .spawn(async move {
            let mut reader = entry.reader(index).await.unwrap();
            let mut first = vec![0; 4 * 1024 * 1024];
            reader.inner.read_exact(&mut first).await.unwrap();
            (reader, first)
        })
        .await
        .unwrap();
    assert_eq!(first, data[..4 * 1024 * 1024]);
    let bound = (4 + 32) * 1024 * 1024 + 64 * 1024;
    timeout(Duration::from_secs(45), async {
        while stream.stats().verified_bytes < 32 * 1024 * 1024 {
            assert!(stream.stats().error.is_none(), "{:?}", stream.stats());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("window did not fill: {:?}", stream.stats()));
    timeout(Duration::from_secs(10), async {
        let mut progress = stream.stats();
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let next = stream.stats();
            assert!(
                next.verified_bytes <= bound,
                "stationary reader exceeded lookahead: {next:?}"
            );
            if next.verified_bytes == progress.verified_bytes
                && next.downloaded_bytes == progress.downloaded_bytes
            {
                break;
            }
            progress = next;
        }
    })
    .await
    .unwrap();
    let before = stream.stats();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after = stream.stats();
    assert_eq!(
        stream
            .playback
            .entry
            .readers
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    println!(
        "stationary window: verified={} fetched={} bound={}",
        after.verified_bytes, after.downloaded_bytes, bound
    );
    assert!(after.verified_bytes <= bound);
    assert_eq!(after.verified_bytes, before.verified_bytes);
    assert_eq!(after.downloaded_bytes, before.downloaded_bytes);
    assert!(after.error.is_none());
    drop(reader);
    timeout(Duration::from_secs(1), async {
        while !stream.playback.entry.torrent.is_paused() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let before = stream.stats().verified_bytes;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(stream.stats().verified_bytes, before);
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nonreading_client_with_live_fetching_has_bounded_shutdown() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("blocked.mp4", 32 * 1024 * 1024).await;
    let cache = swarm.dir.path().join("cache");
    let engine = swarm.engine(cache.clone(), 64 * 1024 * 1024);
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    let mut client = connect(&stream.url, None, None).await;
    let mut prefix = [0; 8192];
    client.read_exact(&mut prefix).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        stream
            .playback
            .entry
            .readers
            .load(std::sync::atomic::Ordering::Acquire)
            > 0
    );
    assert!(stream.stats().error.is_none());
    let start = Instant::now();
    timeout(Duration::from_secs(6), engine.shutdown())
        .await
        .unwrap()
        .unwrap();
    println!("blocked-client shutdown: {:?}", start.elapsed());
    assert!(!cache.exists());
    assert!(stream.playback.entry.session.upgrade().is_none());
    drop(client);
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_peers_after_metadata_is_reported_in_stats() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("unseeded.mp4", 8 * 1024 * 1024).await;
    let engine = TorrentEngine::offline(
        TorrentOptions {
            cache_dir: swarm.dir.path().join("cache"),
            no_peers_timeout: Duration::from_millis(300),
            metadata_timeout: Duration::from_secs(5),
            ..Default::default()
        },
        vec![swarm.peer],
    );
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    assert!(stream.stats().verified_bytes < stream.file_size);
    swarm.seeder.stop().await;
    let mut client = connect(&stream.url, Some("bytes=8388508-8388607"), None).await;
    let mut body = Vec::new();
    let _ = timeout(Duration::from_secs(2), client.read_to_end(&mut body))
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        while stream.stats().error.is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(stream.stats().state, TorrentState::Stalled);
    assert_eq!(stream.stats().error, Some(TorrentError::NoPeers));
    engine.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_file_index_no_video_and_free_space_failure() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("notes.txt", 1024 * 1024).await;
    let engine = swarm.engine(swarm.dir.path().join("cache"), 8 * 1024 * 1024);
    assert_eq!(
        engine
            .open(source.clone(), CancellationToken::new())
            .await
            .err(),
        Some(TorrentError::NoVideoFile)
    );
    let indexed = TorrentSource::from_stream(source.info_hash(), Some(9), &[]).unwrap();
    assert_eq!(
        engine.open(indexed, CancellationToken::new()).await.err(),
        Some(TorrentError::InvalidSource)
    );
    let indexed = TorrentSource::from_stream(source.info_hash(), Some(0), &[]).unwrap();
    let stream = engine
        .open(indexed, CancellationToken::new())
        .await
        .unwrap();
    let (other, _) = swarm.video("other.mp4", 1024 * 1024).await;
    let host = engine.host_for_test().await;
    lock(&host.shared.budget).free_override = Some(0);
    assert_eq!(
        engine.open(other, CancellationToken::new()).await.err(),
        Some(TorrentError::Disk(DiskError::Full))
    );
    assert_eq!(get(&stream.url, Some("bytes=0-99"), None).await.0, 206);
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_invalidates_the_returned_lease() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("cancel.mp4", 1024 * 1024).await;
    let engine = swarm.engine(swarm.dir.path().join("cache"), 8 * 1024 * 1024);
    let cancel = CancellationToken::new();
    let stream = engine.open(source, cancel.clone()).await.unwrap();
    cancel.cancel();
    timeout(Duration::from_secs(1), async {
        while lock(&engine.host_for_test().await.shared.routes).contains_key(&stream.token) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(get(&stream.url, None, None).await.0, 404);
    assert!(stream.playback.entry.torrent.is_paused());
    engine.shutdown().await.unwrap();
    stream.close().await.unwrap();
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn largest_video_only_and_multiple_files_share_a_torrent() {
    let swarm = Swarm::new().await;
    let (source, data, small) = swarm.bundle().await;
    let engine = swarm.engine(swarm.dir.path().join("cache"), 16 * 1024 * 1024);
    let large = engine
        .open(source.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(large.file_name, "large.MKV");
    assert_eq!(get(&large.url, None, None).await.2, data);
    assert_eq!(large.playback.entry.torrent.stats().file_progress[small], 0);
    let source = TorrentSource::from_stream(source.info_hash(), Some(small as u32), &[]).unwrap();
    let small = engine.open(source, CancellationToken::new()).await.unwrap();
    assert_eq!(small.file_name, "small.mp4");
    assert!(std::sync::Arc::ptr_eq(
        &large.playback.entry,
        &small.playback.entry
    ));
    assert_eq!(get(&small.url, None, None).await.2, vec![17; 512 * 1024]);
    large.close().await.unwrap();
    assert_eq!(
        get(&small.url, Some("bytes=0-9"), None).await.2,
        vec![17; 10]
    );
    small.close().await.unwrap();
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_pending_open_releases_its_operation_and_idle_session() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let peer = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let engine = std::sync::Arc::new(TorrentEngine::offline(
        TorrentOptions {
            cache_dir: cache.clone(),
            metadata_timeout: Duration::from_secs(5),
            idle_stop_after: Duration::from_millis(100),
            ..Default::default()
        },
        vec![peer.local_addr().unwrap()],
    ));
    let task_engine = engine.clone();
    let task = tokio::spawn(async move {
        task_engine
            .open(
                TorrentSource::from_stream("0000000000000000000000000000000000000001", None, &[])
                    .unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (_connection, _) = timeout(Duration::from_secs(3), peer.accept())
        .await
        .unwrap()
        .unwrap();
    let host = engine.host_for_test().await;
    assert!(!task.is_finished(), "open completed before cancellation");
    assert_eq!(
        host.shared
            .operations
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    task.abort();
    let _ = task.await;
    timeout(Duration::from_secs(4), async {
        while !host.shared.stop.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        host.shutdown().await.unwrap();
    })
    .await
    .unwrap();
    assert!(!cache.exists());
    assert_eq!(
        host.shared
            .operations
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_a_nonreading_lease_preserves_another_lease_and_releases_readers() {
    let swarm = Swarm::new().await;
    let (source, data) = swarm.video("close.mp4", 20 * 1024 * 1024).await;
    let engine = swarm.engine(swarm.dir.path().join("cache"), 32 * 1024 * 1024);
    let first = engine
        .open(source.clone(), CancellationToken::new())
        .await
        .unwrap();
    let mut client = connect(&first.url, None, None).await;
    let mut prefix = [0; 8192];
    client.read_exact(&mut prefix).await.unwrap();
    let second = engine.open(source, CancellationToken::new()).await.unwrap();
    let (bytes, closed) = tokio::join!(
        get(&second.url, Some("bytes=10485760-10485859"), None),
        first.close()
    );
    closed.unwrap();
    assert_eq!(bytes.0, 206, "{:?}", second.stats());
    assert_eq!(bytes.2, data[10485760..10485860], "{:?}", second.stats());
    tokio::time::timeout(Duration::from_secs(1), async {
        while first
            .playback
            .entry
            .readers
            .load(std::sync::atomic::Ordering::Acquire)
            != 0
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(get(&first.url, None, None).await.0, 404);
    second.close().await.unwrap();
    assert!(second.playback.entry.torrent.is_paused());
    engine.shutdown().await.unwrap();
    drop(client);
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn low_free_space_evicts_inactive_torrents_but_protects_open_leases() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("space-a.mp4", 256 * 1024).await;
    let (other, _) = swarm.video("space-b.mp4", 512 * 1024).await;
    let cache = swarm.dir.path().join("cache");
    let engine = swarm.engine(cache.clone(), 8 * 1024 * 1024);
    let stream = engine
        .open(source.clone(), CancellationToken::new())
        .await
        .unwrap();
    let host = engine.host_for_test().await;
    lock(&host.shared.budget).free_override = Some(0);
    assert_eq!(
        engine
            .open(other.clone(), CancellationToken::new())
            .await
            .err(),
        Some(TorrentError::Disk(DiskError::Full))
    );
    assert!(cache.join(source.info_hash()).exists());
    stream.close().await.unwrap();
    assert_eq!(
        engine.open(other, CancellationToken::new()).await.err(),
        Some(TorrentError::Disk(DiskError::Full))
    );
    assert!(!cache.join(source.info_hash()).exists());
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metadata_deadline_includes_waiting_for_previous_runtime_teardown() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("deadline.mp4", 64 * 1024).await;
    let engine = TorrentEngine::offline(
        TorrentOptions {
            cache_dir: swarm.dir.path().join("cache"),
            metadata_timeout: Duration::from_secs(1),
            ..Default::default()
        },
        vec![swarm.peer],
    );
    let stream = engine
        .open(source.clone(), CancellationToken::new())
        .await
        .unwrap();
    let host = engine.host_for_test().await;
    let completion = host.completion.lock().await;
    host.shared.stop.cancel();
    assert_eq!(
        timeout(
            Duration::from_secs(2),
            engine.open(source, CancellationToken::new())
        )
        .await
        .unwrap()
        .err(),
        Some(TorrentError::MetadataTimeout)
    );
    drop(completion);
    engine.shutdown().await.unwrap();
    stream.close().await.unwrap();
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn existing_cache_is_not_adopted_or_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("existing");
    std::fs::create_dir(&cache).unwrap();
    std::fs::write(cache.join("sentinel"), b"keep").unwrap();
    let engine = TorrentEngine::offline(
        TorrentOptions {
            cache_dir: cache.clone(),
            ..Default::default()
        },
        vec![],
    );
    let source =
        TorrentSource::from_stream("0000000000000000000000000000000000000001", None, &[]).unwrap();
    assert_eq!(
        engine.open(source, CancellationToken::new()).await.err(),
        Some(TorrentError::Disk(DiskError::Io))
    );
    engine.shutdown().await.unwrap();
    assert_eq!(std::fs::read(cache.join("sentinel")).unwrap(), b"keep");
}
