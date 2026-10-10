use super::super::*;
use super::fixture::{Swarm, get};
use std::sync::Arc;
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_payload_does_not_block_a_cached_torrent() {
    let swarm = Swarm::new().await;
    let (cached, data) = swarm.video("cached.mp4", 64 * 1024).await;
    let (stalled, _) = swarm.video("corrupt.mp4", 64 * 1024).await;
    std::fs::write(swarm.dir.path().join("corrupt.mp4"), vec![0; 64 * 1024]).unwrap();
    let hash = stalled.info_hash().to_owned();
    let seeding = swarm.seeder.with_torrents(|torrents| {
        for (_, torrent) in torrents {
            if torrent.info_hash().as_string() == hash {
                return torrent.clone();
            }
        }
        panic!("missing seeder torrent");
    });
    let engine = Arc::new(swarm.engine(swarm.dir.path().join("cache"), 8 * 1024 * 1024));
    let first = engine
        .open(cached.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(get(&first.url, None, None).await.2, data);
    first.close().await.unwrap();
    let task_engine = engine.clone();
    let pending =
        tokio::spawn(async move { task_engine.open(stalled, CancellationToken::new()).await });
    timeout(Duration::from_secs(3), async {
        while seeding.stats().uploaded_bytes == 0 {
            assert!(!pending.is_finished());
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!pending.is_finished());
    let opened = timeout(
        Duration::from_millis(500),
        engine.open(cached, CancellationToken::new()),
    )
    .await;
    pending.abort();
    let _ = pending.await;
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
    opened.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simultaneous_opens_share_one_initialization() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("shared.mp4", 64 * 1024).await;
    let engine = swarm.engine(swarm.dir.path().join("cache"), 8 * 1024 * 1024);
    let (first, second) = tokio::join!(
        engine.open(source.clone(), CancellationToken::new()),
        engine.open(source, CancellationToken::new())
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert!(Arc::ptr_eq(&first.playback.entry, &second.playback.entry));
    assert_eq!(
        engine
            .host_for_test()
            .await
            .shared
            .state
            .lock()
            .await
            .torrents
            .len(),
        1
    );
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
}
