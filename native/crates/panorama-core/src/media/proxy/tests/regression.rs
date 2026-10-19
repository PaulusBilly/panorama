use super::*;
use crate::media::cache::MediaCache;
use session::{Chunk, ChunkState};
use std::sync::atomic::AtomicBool;
use tokio::sync::{Notify, watch};

#[tokio::test(start_paused = true)]
async fn old_chunk_drop_preserves_replacement_reservation_and_cache_stats() {
    let fixture = Fixture::new();
    let (_dir, mut proxy, handle) = drive(setup(&fixture, false)).await;
    let cache = handle.session.cache.clone();
    let key = handle.session.key(7);
    let chunk = |lease| Chunk {
        data: Mutex::new(ChunkState {
            data: Default::default(),
            complete: None,
            error: None,
        }),
        notify: Notify::new(),
        stop: watch::channel(false).0,
        key: key.clone(),
        cache: cache.clone(),
        lease: Mutex::new(Some(lease)),
        running: AtomicBool::new(false),
    };
    let old_lease = cache.reserve(&key, CHUNK_BYTES).unwrap().unwrap();
    let old = chunk(old_lease);
    cache.release(&key, old_lease);
    let replacement_lease = cache.reserve(&key, CHUNK_BYTES).unwrap().unwrap();
    assert_ne!(old_lease, replacement_lease);
    let replacement = chunk(replacement_lease);
    let before = cache.stats();
    assert_eq!(before.reserved_bytes, CHUNK_BYTES);
    drop(old);
    assert_eq!(cache.stats(), before);
    assert_eq!(cache.reserve(&key, CHUNK_BYTES), Ok(None));
    drop(replacement);
    assert_eq!(cache.stats().reserved_bytes, 0);
    drive(proxy.close()).await;
}

#[tokio::test(start_paused = true)]
async fn cache_and_proxy_reject_memory_budgets_below_one_chunk() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    for bytes in [0, CHUNK_BYTES - 1] {
        let mut options = ProxyOptions::default();
        options.cache.directory = dir.path().join("cache");
        options.cache.max_memory_bytes = bytes;
        assert!(matches!(
            MediaCache::open(options.cache.clone()).await,
            Err(MediaError::MemoryBudget)
        ));
        assert!(matches!(
            MediaProxy::with_fetch(options, Arc::new(fixture.clone()), None).await,
            Err(MediaError::MemoryBudget)
        ));
        assert!(!dir.path().join("cache").exists());
    }
    assert_eq!(
        MediaError::MemoryBudget.to_string(),
        "Media cache memory budget must be at least 2097152 bytes"
    );
}

#[tokio::test(start_paused = true)]
async fn oversized_chunk_admission_is_terminal_and_preserves_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = ProxyOptions::default().cache;
    options.directory = dir.path().to_owned();
    options.max_memory_bytes = CHUNK_BYTES;
    let cache = drive(MediaCache::open(options)).await.unwrap();
    let before = cache.stats();
    assert_eq!(
        cache.reserve("session:oversized", CHUNK_BYTES + 1),
        Err(MediaError::ChunkTooLarge)
    );
    assert_eq!(cache.stats(), before);
    assert_eq!(
        MediaError::ChunkTooLarge.to_string(),
        "Media chunk exceeds cache memory budget"
    );
    let lease = cache.reserve("session:fits", CHUNK_BYTES).unwrap().unwrap();
    assert_eq!(cache.reserve("session:pressure", 1), Ok(None));
    cache.release("session:fits", lease);
    drive(cache.close()).await;
}
