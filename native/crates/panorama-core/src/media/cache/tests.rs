//! Test equivalents from `tests/unit/media-cache.test.ts`.
use super::*;
use tempfile::{TempDir, tempdir};

async fn memory_cache() -> (TempDir, MediaCache) {
    let dir = tempdir().unwrap();
    let file = dir.path().join("not-a-directory");
    fs::write(&file, b"fixture").unwrap();
    let cache = MediaCache::open(MediaCacheOptions {
        directory: file,
        max_memory_bytes: super::super::proxy::CHUNK_BYTES,
        max_disk_bytes: 16384,
        reserve_free_bytes: 0,
    })
    .await
    .unwrap();
    lock(&cache.state).options.max_memory_bytes = 16;
    (dir, cache)
}
async fn disk_cache(reserve_free_bytes: u64) -> (TempDir, MediaCache) {
    let dir = tempdir().unwrap();
    let cache = MediaCache::open(MediaCacheOptions {
        directory: dir.path().to_owned(),
        max_memory_bytes: super::super::proxy::CHUNK_BYTES,
        max_disk_bytes: 20480,
        reserve_free_bytes,
    })
    .await
    .unwrap();
    lock(&cache.state).options.max_memory_bytes = 16384;
    (dir, cache)
}
#[tokio::test]
async fn reserves_memory_before_admission_and_evicts_unpinned_completed_entries() {
    let (_dir, cache) = memory_cache().await;
    let lease = cache.reserve("inflight", 16).unwrap().unwrap();
    assert_eq!(cache.reserve("overflow", 1), Ok(None));
    assert_eq!(cache.stats().reserved_bytes, 16);
    cache.release("inflight", lease);
    cache.put("a", Bytes::from(vec![1; 8])).await.unwrap();
    cache.put("b", Bytes::from(vec![2; 8])).await.unwrap();
    assert_eq!(cache.get("a").await, None);
    assert_eq!(cache.get("b").await, Some(Bytes::from(vec![2; 8])));
    assert!(cache.stats().memory_bytes + cache.stats().reserved_bytes <= 16);
}
#[tokio::test]
async fn preserves_reference_counted_pins_and_rejects_writes_under_pressure() {
    let (_dir, cache) = memory_cache().await;
    cache.put("a", Bytes::from(vec![0; 8])).await.unwrap();
    cache.pin("a");
    cache.pin("a");
    assert!(
        cache
            .put("b", Bytes::from(vec![0; 8]))
            .await
            .unwrap_err()
            .to_string()
            .contains("admission")
    );
    cache.unpin("a");
    assert_eq!(cache.reserve("pressure", 9), Ok(None));
    cache.unpin("a");
    let lease = cache.reserve("pressure", 9).unwrap().unwrap();
    cache.release("pressure", lease);
}
#[tokio::test]
async fn removes_a_retired_session_after_its_final_reader_releases_the_pin() {
    let (_dir, cache) = memory_cache().await;
    cache
        .put("session:a", Bytes::from(vec![0; 8]))
        .await
        .unwrap();
    cache.pin("session:a");
    cache.remove_session("session:").await;
    assert!(cache.has("session:a"));
    cache.unpin("session:a");
    cache.remove_session("session:").await;
    assert!(!cache.has("session:a"));
    assert_eq!(cache.stats().memory_bytes, 0);
}
#[tokio::test]
async fn caps_allocated_disk_bytes_and_removes_only_its_owned_directory() {
    let (dir, cache) = disk_cache(0).await;
    fs::write(dir.path().join("foreign"), b"keep").unwrap();
    for index in 0..5 {
        cache
            .put(&index.to_string(), Bytes::from(vec![index as u8; 4096]))
            .await
            .unwrap();
        assert!(cache.stats().disk_bytes <= 20480);
    }
    cache.close().await;
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["foreign"]);
}
#[tokio::test]
async fn keeps_validated_bytes_in_bounded_memory_when_its_disk_write_fails() {
    let (_dir, cache) = disk_cache(0).await;
    let owned = lock(&cache.state).disk.as_ref().unwrap().path.clone();
    fs::create_dir(owned.join(format!("{:x}", sha2::Sha256::digest(b"fallback")))).unwrap();
    cache
        .put("fallback", Bytes::from(vec![7; 4096]))
        .await
        .unwrap();
    assert_eq!(
        cache.get("fallback").await,
        Some(Bytes::from(vec![7; 4096]))
    );
    assert_eq!(
        cache.stats(),
        CacheStats {
            memory_bytes: 4096,
            disk_bytes: 4096,
            reserved_bytes: 0
        }
    );
}
#[tokio::test]
async fn preserves_the_free_space_reserve_without_losing_foreground_bytes() {
    let (_dir, cache) = disk_cache(super::super::range::MAX_SAFE_INTEGER).await;
    cache
        .put("foreground", Bytes::from(vec![3; 4096]))
        .await
        .unwrap();
    assert_eq!(
        cache.get("foreground").await,
        Some(Bytes::from(vec![3; 4096]))
    );
    assert_eq!(cache.stats().disk_bytes, 4096);
    assert_eq!(cache.stats().reserved_bytes, 0);
}
#[tokio::test]
async fn cleans_stale_owners_and_preserves_live_owners_foreign_files_and_symlinks() {
    let dir = tempdir().unwrap();
    let stale = dir.path().join("owner-99999-abcdef-old");
    fs::create_dir(&stale).unwrap();
    fs::write(stale.join("owner.lock"), []).unwrap();
    fs::write(stale.join("chunk"), b"old").unwrap();
    let foreign = dir.path().join("foreign");
    fs::write(&foreign, b"keep").unwrap();
    let unrelated = dir.path().join("unrelated-directory");
    fs::create_dir(&unrelated).unwrap();
    let owner_file = dir.path().join("owner-99999-abcdef-file");
    fs::write(&owner_file, b"keep").unwrap();
    #[cfg(unix)]
    let linked = {
        let linked = dir.path().join("owner-99999-abcdef-link");
        std::os::unix::fs::symlink(&unrelated, &linked).unwrap();
        linked
    };
    let options = MediaCacheOptions {
        directory: dir.path().to_owned(),
        max_memory_bytes: super::super::proxy::CHUNK_BYTES,
        max_disk_bytes: 20480,
        reserve_free_bytes: 0,
    };
    let first = MediaCache::open(options.clone()).await.unwrap();
    let live = lock(&first.state).disk.as_ref().unwrap().path.clone();
    let second = MediaCache::open(options).await.unwrap();
    assert!(!stale.exists());
    assert!(live.exists());
    assert!(foreign.exists());
    assert!(unrelated.exists());
    assert!(owner_file.exists());
    #[cfg(unix)]
    assert!(linked.exists());
    second.close().await;
    assert!(live.exists());
    first.close().await;
    assert!(!live.exists());
}
#[tokio::test]
async fn close_rejects_admission_and_lru_disk_reads_recover_missing_files() {
    let (_dir, cache) = disk_cache(0).await;
    lock(&cache.state).options.max_disk_bytes = 32768;
    cache.put("a", Bytes::from(vec![1; 4096])).await.unwrap();
    cache.put("b", Bytes::from(vec![2; 8192])).await.unwrap();
    assert_eq!(cache.get("a").await, Some(Bytes::from(vec![1; 4096])));
    if let Some(file) = lock(&cache.state)
        .entries
        .get("a")
        .and_then(|entry| entry.file.clone())
    {
        fs::remove_file(file).unwrap();
    }
    assert_eq!(cache.get("a").await, None);
    assert!(!cache.has("a"));
    cache.close().await;
    assert_eq!(cache.reserve("new", 1), Err(MediaError::Closed));
    assert!(cache.put("new", Bytes::from_static(b"x")).await.is_err());
    assert_eq!(cache.get("a").await, None);
    assert_eq!(cache.stats(), CacheStats::default());
}

use sha2::Digest;

#[tokio::test]
async fn disk_allocation_rejects_insufficient_free_space_even_without_reserve() {
    let (_dir, cache) = disk_cache(0).await;
    let state = lock(&cache.state);
    let disk = state.disk.as_ref().unwrap();
    assert_eq!(disk.allocation(u64::MAX / 2, 0), None);
}
