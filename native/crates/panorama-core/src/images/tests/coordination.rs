use std::sync::{Arc, atomic::Ordering};

use futures::future::join_all;

use super::*;

#[tokio::test]
async fn twenty_waiters_share_one_download() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let loader = server.loader(options(dir.path()));
    let requests = (0..20)
        .map(|_| request(&server, "/slow/shared", 20, 20))
        .collect::<Vec<_>>();
    let task = tokio::spawn(async move {
        join_all(requests.into_iter().map(|request| loader.load(request))).await
    });
    server.wait_requests(1).await;
    server.counts.release.add_permits(1);
    for result in task.await.unwrap() {
        assert!(result.is_ok());
    }
    assert_eq!(server.requests(), 1);
}

#[tokio::test]
async fn twenty_distinct_urls_respect_download_cap() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = options(dir.path());
    options.max_concurrent = 3;
    let loader = server.loader(options);
    let requests = (0..20)
        .map(|i| request(&server, &format!("/slow/{i}"), 10, 10))
        .collect::<Vec<_>>();
    let task = tokio::spawn(async move {
        join_all(requests.into_iter().map(|request| loader.load(request))).await
    });
    server.wait_requests(3).await;
    assert_eq!(server.requests(), 3);
    assert_eq!(server.counts.peak.load(Ordering::SeqCst), 3);
    server.counts.release.add_permits(20);
    for result in task.await.unwrap() {
        assert!(result.is_ok());
    }
    assert_eq!(server.requests(), 20);
    assert_eq!(server.counts.peak.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn dropping_last_waiter_releases_download_slot_and_starts_new_flight() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = options(dir.path());
    options.max_concurrent = 1;
    let loader = server.loader(options);
    let shared = loader.clone();
    let first_request = request(&server, "/slow/cancel", 10, 10);
    let first = tokio::spawn(async move { shared.load(first_request).await });
    server.wait_requests(1).await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let next_request = request(&server, "/slow/cancel", 10, 10);
    let next = tokio::spawn(async move { loader.load(next_request).await });
    server.wait_requests(2).await;
    server.counts.release.add_permits(2);
    assert!(next.await.unwrap().is_ok());
    assert_eq!(server.requests(), 2);
}

#[tokio::test]
async fn dropping_one_waiter_keeps_other_waiter_interested() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let loader = Arc::new(server.loader(options(dir.path())));
    let load = request(&server, "/slow/keep", 10, 10);
    let first_loader = Arc::clone(&loader);
    let first_request = load.clone();
    let first = tokio::spawn(async move { first_loader.load(first_request).await });
    server.wait_requests(1).await;
    let mut second = Box::pin(loader.load(load));
    assert!(futures::poll!(second.as_mut()).is_pending());
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    server.counts.release.add_permits(1);
    assert!(second.await.is_ok());
    assert_eq!(server.requests(), 1);
}

#[tokio::test]
async fn clear_during_download_prevents_old_insert() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let config = options(dir.path());
    let loader = server.loader(config.clone());
    let shared = loader.clone();
    let load = request(&server, "/slow/clear", 10, 10);
    let task = tokio::spawn(async move { shared.load(load).await });
    server.wait_requests(1).await;
    loader.clear().await.unwrap();
    server.counts.release.add_permits(1);
    assert!(task.await.unwrap().is_ok());
    assert_eq!(std::fs::read_dir(config.cache_dir).unwrap().count(), 0);
}

#[tokio::test]
async fn clear_before_scheduled_cache_read_prevents_old_insert() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let config = options(dir.path());
    let mut loader = server.loader(config.clone());
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = std::sync::Mutex::new(release_rx);
    loader.before_read(Arc::new(move || {
        entered_tx.send(()).unwrap();
        release_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    }));
    let shared = loader.clone();
    let load = request(&server, "/png", 10, 10);
    let task = tokio::spawn(async move { shared.load(load).await });
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), loader.clear())
        .await
        .unwrap()
        .unwrap();
    release_tx.send(()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    assert_eq!(std::fs::read_dir(config.cache_dir).unwrap().count(), 0);
}
