use super::*;
use crate::media::{
    cache::MediaCache,
    fetch::{HttpMediaFetch, MediaSource},
    proxy::ProxyOptions,
};
use std::sync::Mutex;

#[tokio::test(start_paused = true)]
async fn opening_chunk_larger_than_budget_fails_without_retrying() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = ProxyOptions::default();
    options.cache.directory = dir.path().to_owned();
    options.cache.max_memory_bytes = CHUNK_BYTES;
    let cache = MediaCache::open(options.cache.clone()).await.unwrap();
    let session = Session::new(
        MediaSource::new("http://fixture.invalid/film").unwrap(),
        Arc::new(HttpMediaFetch::new().unwrap()),
        None,
        cache.clone(),
        &options,
        Default::default(),
    )
    .unwrap();
    let chunk = Chunk {
        data: Mutex::new(ChunkState {
            data: BytesMut::new(),
            complete: None,
            error: None,
        }),
        notify: Notify::new(),
        stop: watch::channel(false).0,
        key: session.key(0),
        cache: cache.clone(),
        lease: Mutex::new(None),
        running: AtomicBool::new(false),
    };
    let before = cache.stats();
    let admission = session.admit(0, &chunk, CHUNK_BYTES + 1, true);
    tokio::pin!(admission);
    assert_eq!(
        futures::poll!(&mut admission),
        std::task::Poll::Ready(Err(MediaError::ChunkTooLarge))
    );
    assert!(lock(&chunk.lease).is_none());
    assert_eq!(cache.stats(), before);
    cache.close().await;
}
