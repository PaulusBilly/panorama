use std::{fs, sync::Arc};

use futures::future::join_all;

use super::*;

fn files(root: &Path) -> Vec<std::path::PathBuf> {
    fs::read_dir(root)
        .unwrap()
        .flat_map(|entry| {
            let entry = entry.unwrap();
            fs::read_dir(entry.path())
                .unwrap()
                .map(|file| file.unwrap().path())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn total(root: &Path) -> u64 {
    files(root)
        .iter()
        .map(|file| fs::metadata(file).unwrap().len())
        .sum()
}

fn path(config: &ImageLoaderOptions, load: &ImageRequest) -> std::path::PathBuf {
    let key = super::super::cache::Cache::key(&load.url);
    config.cache_dir.join(&key[..2]).join(key)
}

#[tokio::test]
async fn disk_hit_reuses_original_for_other_targets_and_offline_restart() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let config = options(dir.path());
    let loader = server.loader(config.clone());
    let first = request(&server, "/png", 20, 20);
    assert_eq!(loader.load(first.clone()).await.unwrap().width, 20);
    let mut second = first.clone();
    second.target = ImageTarget {
        max_width: 60,
        max_height: 60,
    };
    assert_eq!(loader.load(second.clone()).await.unwrap().width, 60);
    assert_eq!(server.requests(), 1);
    assert_eq!(
        fs::read(path(&config, &first)).unwrap(),
        encoded(ImageFormat::Png, 80, 40)
    );
    drop(loader);
    drop(server);
    let loader = ImageLoader::new(config).unwrap();
    assert_eq!(loader.load(second).await.unwrap().width, 60);
}

#[tokio::test]
async fn budget_evicts_lru_touches_reads_and_rebuilds_on_restart() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = encoded(ImageFormat::Png, 80, 40).len() as u64;
    let mut config = options(dir.path());
    config.max_disk_bytes = size * 3;
    let loader = server.loader(config.clone());
    let loads = (0..5)
        .map(|i| request(&server, &format!("/png/{i}"), 20, 20))
        .collect::<Vec<_>>();
    for load in &loads[..3] {
        loader.load(load.clone()).await.unwrap();
    }
    loader.load(loads[0].clone()).await.unwrap();
    drop(loader);
    let loader = server.loader(config.clone());
    assert_eq!(total(&config.cache_dir), size * 3);
    loader.load(loads[3].clone()).await.unwrap();
    assert!(path(&config, &loads[0]).exists());
    assert!(!path(&config, &loads[1]).exists());
    assert!(!path(&config, &loads[2]).exists());
    assert!(path(&config, &loads[3]).exists());
    assert!(total(&config.cache_dir) <= config.max_disk_bytes * 9 / 10);
    drop(loader);
    config.max_disk_bytes = size;
    let _loader = server.loader(config.clone());
    assert_eq!(total(&config.cache_dir), 0);
}

#[tokio::test]
async fn concurrent_loaders_share_budget_without_corrupting_files() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = options(dir.path());
    config.max_disk_bytes = encoded(ImageFormat::Png, 80, 40).len() as u64 * 4;
    let first = Arc::new(server.loader(config.clone()));
    let second = Arc::new(server.loader(config.clone()));
    let tasks = (0..20).map(|i| {
        let loader = if i % 2 == 0 {
            Arc::clone(&first)
        } else {
            Arc::clone(&second)
        };
        let load = request(&server, &format!("/png/{i}"), 10, 10);
        tokio::spawn(async move { loader.load(load).await })
    });
    for result in join_all(tasks).await {
        assert!(result.unwrap().is_ok());
    }
    assert!(total(&config.cache_dir) <= config.max_disk_bytes);
    for file in files(&config.cache_dir) {
        assert!(super::super::decode::decode(&fs::read(file).unwrap()).is_ok());
    }
}

#[tokio::test]
async fn corrupted_disk_hit_refetches_once_and_failed_refetch_stays_deleted() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let config = options(dir.path());
    let loader = server.loader(config.clone());
    let load = request(&server, "/png", 20, 20);
    loader.load(load.clone()).await.unwrap();
    fs::write(path(&config, &load), b"truncated").unwrap();
    loader.load(load.clone()).await.unwrap();
    assert_eq!(server.requests(), 2);
    let unsupported = request(&server, "/unsupported", 20, 20);
    let file = path(&config, &unsupported);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"corrupt").unwrap();
    assert_eq!(
        loader.load(unsupported).await.unwrap_err(),
        ImageError::Unsupported
    );
    assert!(!file.exists());
    assert_eq!(server.requests(), 3);
}

#[tokio::test]
async fn clear_empties_cache_and_forces_a_new_download() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let config = options(dir.path());
    let loader = server.loader(config.clone());
    let load = request(&server, "/png", 10, 10);
    loader.load(load.clone()).await.unwrap();
    loader.clear().await.unwrap();
    assert_eq!(fs::read_dir(&config.cache_dir).unwrap().count(), 0);
    loader.load(load).await.unwrap();
    assert_eq!(server.requests(), 2);
}

#[tokio::test]
async fn oversized_cache_entry_is_refetched_and_zero_budget_retains_nothing() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = options(dir.path());
    config.max_body_bytes = 1024;
    let loader = server.loader(config.clone());
    let load = request(&server, "/png", 20, 20);
    loader.load(load.clone()).await.unwrap();
    fs::write(path(&config, &load), vec![0; 1025]).unwrap();
    loader.load(load.clone()).await.unwrap();
    assert_eq!(server.requests(), 2);
    drop(loader);
    config.max_disk_bytes = 0;
    let loader = server.loader(config.clone());
    loader.load(load).await.unwrap();
    assert_eq!(total(&config.cache_dir), 0);
}

#[test]
fn restart_removes_abandoned_temporary_files() {
    let dir = tempfile::tempdir().unwrap();
    let config = options(dir.path());
    fs::create_dir_all(config.cache_dir.join("ab")).unwrap();
    fs::write(config.cache_dir.join("ab/.tmp-1-1"), b"partial").unwrap();
    let _loader = ImageLoader::new(config.clone()).unwrap();
    assert_eq!(files(&config.cache_dir).len(), 0);
}
