use super::super::*;
use super::fixture::{Swarm, get};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tracker_only_metadata_uses_a_dht_disabled_session() {
    let swarm = Swarm::new().await;
    let (source, data) = swarm.video("private.mp4", 64 * 1024).await;
    let tracker = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tracker_port = tracker.local_addr().unwrap().port();
    let tracker_url = format!("tracker:http://{}/announce", tracker.local_addr().unwrap());
    let (announced, mut announces) = tokio::sync::mpsc::unbounded_channel();
    let peer = swarm.peer;
    let worker = tokio::spawn(async move {
        loop {
            let (mut connection, _) = tracker.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(connection.read_u8().await.unwrap());
            }
            assert!(String::from_utf8_lossy(&request).contains("/announce?"));
            let mut body = b"d8:intervali1e5:peers6:".to_vec();
            body.extend_from_slice(&[127, 0, 0, 1]);
            body.extend_from_slice(&peer.port().to_be_bytes());
            body.push(b'e');
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            connection.write_all(headers.as_bytes()).await.unwrap();
            connection.write_all(&body).await.unwrap();
            let _ = announced.send(());
        }
    });
    let source = TorrentSource::from_stream(source.info_hash(), None, &[tracker_url]).unwrap();
    let bootstrap = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut engine = TorrentEngine::offline_dht(
        TorrentOptions {
            cache_dir: swarm.dir.path().join("cache"),
            listen_port: Some(0),
            ..Default::default()
        },
        bootstrap.local_addr().unwrap(),
    );
    engine.trackers_for_test();
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    assert!(
        stream
            .playback
            .entry
            .session
            .upgrade()
            .unwrap()
            .get_dht()
            .is_none()
    );
    assert_eq!(get(&stream.url, None, None).await.2, data);
    timeout(Duration::from_secs(1), announces.recv())
        .await
        .unwrap()
        .unwrap();
    let host = engine.host_for_test().await;
    let state = host.shared.state.lock().await;
    assert!(state.session.is_none());
    assert!(state.tracker_session.as_ref().unwrap().get_dht().is_none());
    assert!(
        state
            .tracker_session
            .as_ref()
            .unwrap()
            .listen_addr()
            .is_some()
    );
    drop(state);
    let mut packet = [0; 2048];
    assert!(
        timeout(Duration::from_millis(100), bootstrap.recv_from(&mut packet))
            .await
            .is_err()
    );
    let (public, _) = swarm.video("public.mp4", 64 * 1024).await;
    let public = TorrentSource::from_stream(
        public.info_hash(),
        None,
        &[
            format!("tracker:http://127.0.0.1:{}/announce", tracker_port),
            format!("dht:{}", public.info_hash()),
        ],
    )
    .unwrap();
    let public = engine.open(public, CancellationToken::new()).await.unwrap();
    assert!(
        public
            .playback
            .entry
            .session
            .upgrade()
            .unwrap()
            .get_dht()
            .is_some()
    );
    assert!(
        public
            .playback
            .entry
            .session
            .upgrade()
            .unwrap()
            .listen_addr()
            .is_none()
    );
    timeout(Duration::from_secs(1), bootstrap.recv_from(&mut packet))
        .await
        .unwrap()
        .unwrap();
    engine.shutdown().await.unwrap();
    worker.abort();
    let _ = worker.await;
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_metadata_does_not_block_a_cached_torrent() {
    let swarm = Swarm::new().await;
    let (source, data) = swarm.video("cached.mp4", 64 * 1024).await;
    let stalled = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let engine = Arc::new(TorrentEngine::offline(
        TorrentOptions {
            cache_dir: swarm.dir.path().join("cache"),
            metadata_timeout: Duration::from_secs(3),
            ..Default::default()
        },
        vec![swarm.peer, stalled.local_addr().unwrap()],
    ));
    let cached = engine
        .open(source.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(get(&cached.url, None, None).await.2, data);
    cached.close().await.unwrap();
    while let Ok(Ok(_)) = timeout(Duration::from_millis(10), stalled.accept()).await {}
    let task_engine = engine.clone();
    let pending = tokio::spawn(async move {
        task_engine
            .open(
                TorrentSource::from_stream("0000000000000000000000000000000000000001", None, &[])
                    .unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (_connection, _) = timeout(Duration::from_secs(2), stalled.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(!pending.is_finished());
    let opened = timeout(
        Duration::from_millis(500),
        engine.open(source, CancellationToken::new()),
    )
    .await;
    pending.abort();
    let _ = pending.await;
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
    assert!(opened.is_ok(), "cached open waited for unrelated metadata");
    opened.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inactive_no_peers_entry_can_be_reopened() {
    let swarm = Swarm::new().await;
    let (source, data) = swarm.video("retry.mp4", 64 * 1024 * 1024).await;
    let engine = TorrentEngine::offline(
        TorrentOptions {
            cache_dir: swarm.dir.path().join("cache"),
            metadata_timeout: Duration::from_secs(5),
            no_peers_timeout: Duration::from_secs(2),
            ..Default::default()
        },
        vec![swarm.peer],
    );
    let first = engine
        .open(source.clone(), CancellationToken::new())
        .await
        .unwrap();
    let seeding = swarm
        .seeder
        .with_torrents(|torrents| torrents.next().unwrap().1.clone());
    swarm.seeder.pause(&seeding).await.unwrap();
    let range = "bytes=67108764-67108863";
    let failed = get(&first.url, Some(range), None).await;
    assert!(failed.2.is_empty());
    assert_eq!(first.stats().error, Some(TorrentError::NoPeers));
    first.close().await.unwrap();
    swarm.seeder.unpause(&seeding).await.unwrap();
    let reopened = engine.open(source, CancellationToken::new()).await.unwrap();
    assert_eq!(reopened.stats().error, None);
    assert_eq!(
        get(&reopened.url, Some(range), None).await.2,
        data[data.len() - 100..]
    );
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
}

#[test]
fn changed_cache_identity_is_never_recursively_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache");
    let budget = super::super::cache::Budget::new(dir.path(), 1024 * 1024).unwrap();
    let cache = super::super::cache::Cache::new(path.clone(), budget).unwrap();
    let replaced = std::fs::rename(&path, dir.path().join("original"));
    #[cfg(windows)]
    if let Err(error) = &replaced {
        assert!(
            matches!(error.raw_os_error(), Some(5) | Some(32)),
            "{error}"
        );
        cache.retire().unwrap();
        assert!(!path.exists());
        return;
    }
    replaced.unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("sentinel"), b"keep").unwrap();
    assert_eq!(cache.retire(), Err(TorrentError::Disk(DiskError::Io)));
    assert_eq!(std::fs::read(path.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn cache_cleanup_refuses_a_nested_reparse_point() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache");
    let target = dir.path().join("external");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("sentinel"), b"keep").unwrap();
    let budget = super::super::cache::Budget::new(dir.path(), 1024 * 1024).unwrap();
    let cache = super::super::cache::Cache::new(path.clone(), budget).unwrap();
    let link = path.join("link");
    #[cfg(windows)]
    {
        assert!(link.starts_with(dir.path()) && target.starts_with(dir.path()));
        assert!(
            std::process::Command::new("cmd.exe")
                .args(["/c", "mklink", "/J"])
                .arg(&link)
                .arg(&target)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_eq!(cache.retire(), Err(TorrentError::Disk(DiskError::Io)));
    assert_eq!(std::fs::read(target.join("sentinel")).unwrap(), b"keep");
    #[cfg(windows)]
    std::fs::remove_dir(link).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(link).unwrap();
    cache.retire().unwrap();
}
