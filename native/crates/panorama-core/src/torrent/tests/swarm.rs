use super::super::*;
use super::fixture::{Swarm, connect, get};
use fs4::fs_std::FileExt;
use tokio::{
    io::AsyncReadExt,
    time::{Instant, timeout},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_swarm_full_ranges_seeks_reuse_security_and_shutdown() {
    let swarm = Swarm::new().await;
    let (source, data) = swarm.video("movie.mp4", 4 * 1024 * 1024).await;
    let cache = swarm.dir.path().join("cache");
    let engine = swarm.engine(cache.clone(), 16 * 1024 * 1024);
    let stream = engine
        .open(source.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(stream.file_name, "movie.mp4");
    assert_eq!(stream.file_size, data.len() as u64);
    assert_eq!(get(&stream.url, None, None).await.2, data);
    for start in [0, data.len() / 2, data.len() * 99 / 100] {
        let end = start + 99;
        let (status, headers, bytes) =
            get(&stream.url, Some(&format!("bytes={start}-{end}")), None).await;
        assert_eq!(status, 206);
        assert!(headers.contains(&format!("bytes {start}-{end}/{}", data.len())));
        assert_eq!(bytes, data[start..=end]);
    }
    let mut first = connect(&stream.url, None, None).await;
    let mut prefix = [0; 1024];
    first.read_exact(&mut prefix).await.unwrap();
    assert_eq!(
        get(&stream.url, Some("bytes=2097152-2097251"), None)
            .await
            .2,
        data[2097152..2097252]
    );
    assert_eq!(get(&stream.url, None, Some("evil.test")).await.0, 404);
    let mut wrong = stream.url.clone();
    wrong.pop();
    wrong.push('g');
    assert_eq!(get(&wrong, None, None).await.0, 404);
    let second = engine.open(source, CancellationToken::new()).await.unwrap();
    assert_eq!(stream.stats().verified_bytes, data.len() as u64);
    let host = engine.host_for_test().await;
    assert_eq!(host.shared.state.lock().await.torrents.len(), 1);
    assert!(
        host.shared
            .state
            .lock()
            .await
            .session
            .as_ref()
            .unwrap()
            .listen_addr()
            .is_none()
    );
    stream.close().await.unwrap();
    assert_eq!(get(&stream.url, None, None).await.0, 404);
    assert_eq!(
        get(&second.url, Some("bytes=0-99"), None).await.2,
        data[..100]
    );
    second.close().await.unwrap();
    assert!(second.playback.entry.torrent.is_paused());
    let address = host.shared.host.clone();
    timeout(Duration::from_secs(6), engine.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(!cache.exists());
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    assert!(host.completion.lock().await.is_none());
    assert!(stream.playback.entry.session.upgrade().is_none());
    drop(first);
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metadata_bound_and_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TorrentEngine::offline(
        TorrentOptions {
            cache_dir: dir.path().join("cache"),
            metadata_timeout: Duration::from_millis(200),
            no_peers_timeout: Duration::from_millis(200),
            ..Default::default()
        },
        vec!["127.0.0.1:1".parse().unwrap()],
    );
    let source =
        TorrentSource::from_stream("0000000000000000000000000000000000000001", None, &[]).unwrap();
    let start = Instant::now();
    assert!(matches!(
        engine
            .open(source.clone(), CancellationToken::new())
            .await
            .err(),
        Some(TorrentError::MetadataTimeout | TorrentError::NoPeers)
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
    let cancel = CancellationToken::new();
    let request = engine.open(source, cancel.clone());
    tokio::pin!(request);
    tokio::select! { _ = &mut request => panic!("request should be waiting"), _ = tokio::time::sleep(Duration::from_millis(20)) => {} }
    cancel.cancel();
    assert_eq!(
        timeout(Duration::from_millis(300), request)
            .await
            .unwrap()
            .err(),
        Some(TorrentError::Cancelled)
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inactive_lru_eviction_protects_open_streams_and_removes_cache() {
    let swarm = Swarm::new().await;
    let (a, data_a) = swarm.video("a.mp4", 2 * 1024 * 1024).await;
    let (b, data_b) = swarm.video("b.mp4", 2 * 1024 * 1024).await;
    let (c, _) = swarm.video("c.mp4", 2 * 1024 * 1024).await;
    let cache = swarm.dir.path().join("cache");
    let engine = swarm.engine(cache.clone(), 4 * 1024 * 1024);
    let first = engine
        .open(a.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(get(&first.url, None, None).await.2, data_a);
    let second = engine.open(b, CancellationToken::new()).await.unwrap();
    assert_eq!(get(&second.url, None, None).await.2, data_b);
    assert_eq!(
        engine.open(c.clone(), CancellationToken::new()).await.err(),
        Some(TorrentError::Disk(DiskError::Full))
    );
    assert!(cache.join(a.info_hash()).exists());
    first.close().await.unwrap();
    let third = engine.open(c, CancellationToken::new()).await.unwrap();
    assert!(!cache.join(a.info_hash()).exists());
    assert_eq!(
        get(&second.url, Some("bytes=0-99"), None).await.2,
        data_b[..100]
    );
    third.close().await.unwrap();
    second.close().await.unwrap();
    engine.shutdown().await.unwrap();
    assert!(!cache.exists());
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_download_scope_and_nonreading_client_shutdown() {
    let swarm = Swarm::new().await;
    let (source, data) = swarm.video("large.mp4", 64 * 1024 * 1024).await;
    let cap = 8 * 1024 * 1024;
    let cache = swarm.dir.path().join("cache");
    let engine = swarm.engine(cache.clone(), cap);
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    let mut client = connect(&stream.url, None, None).await;
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        headers.push(client.read_u8().await.unwrap());
    }
    let mut first = vec![0; 4 * 1024 * 1024];
    timeout(Duration::from_secs(10), client.read_exact(&mut first))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first, data[..first.len()]);
    timeout(Duration::from_secs(15), async {
        while stream.stats().error.is_none() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let actual: u64 = std::fs::read_dir(&stream.playback.entry.cache.path)
        .unwrap()
        .map(|entry| {
            std::fs::File::open(entry.unwrap().path())
                .unwrap()
                .allocated_size()
                .unwrap()
        })
        .sum();
    let stats = stream.stats();
    println!(
        "scope: HTTP=4194304 verified={} fetched={} allocated={} cap={} overshoot={}",
        stats.verified_bytes,
        stats.downloaded_bytes,
        actual,
        cap,
        actual.saturating_sub(cap)
    );
    assert!(actual <= cap);
    assert!(stats.verified_bytes <= cap);
    assert_eq!(stats.error, Some(TorrentError::Disk(DiskError::Full)));
    let start = Instant::now();
    timeout(Duration::from_secs(6), engine.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(start.elapsed() < Duration::from_secs(6));
    assert!(!cache.exists());
    drop(client);
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_stop_and_drop_close_listeners() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("idle.mp4", 1024 * 1024).await;
    let cache = swarm.dir.path().join("cache");
    let engine = TorrentEngine::offline(
        TorrentOptions {
            cache_dir: cache.clone(),
            idle_stop_after: Duration::from_millis(100),
            metadata_timeout: Duration::from_secs(5),
            no_peers_timeout: Duration::from_secs(5),
            ..Default::default()
        },
        vec![swarm.peer],
    );
    let stream = engine
        .open(source.clone(), CancellationToken::new())
        .await
        .unwrap();
    let host = engine.host_for_test().await;
    let address = host.shared.host.clone();
    stream.close().await.unwrap();
    timeout(Duration::from_secs(5), async {
        while !host.shared.stop.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        host.shutdown().await.unwrap();
    })
    .await
    .unwrap();
    assert!(!cache.exists());
    assert!(tokio::net::TcpStream::connect(&address).await.is_err());
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    let address = engine.host_for_test().await.shared.host.clone();
    drop(stream);
    drop(engine);
    timeout(Duration::from_secs(5), async {
        while cache.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    swarm.seeder.stop().await;
}
