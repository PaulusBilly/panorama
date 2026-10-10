use super::{
    CancellationToken, DiskError, TorrentError, TorrentSource, TorrentStream,
    cache::{BLOCK, Cache, Factory},
    lock,
    runtime::{Host, Shared, State},
    stream::{Entry, Playback},
};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions, Session,
    SessionOptions, storage::StorageFactoryExt,
};
use std::{
    num::NonZeroU32,
    sync::{Arc, atomic::Ordering},
};
use tokio::{
    io::AsyncReadExt,
    time::{Instant, timeout_at},
};

async fn session(
    shared: &Shared,
    state: &mut State,
    dht: bool,
) -> Result<Arc<Session>, TorrentError> {
    let listener_in_use = [state.session.as_ref(), state.tracker_session.as_ref()]
        .into_iter()
        .flatten()
        .any(|session| session.listen_addr().is_some());
    let slot = if dht {
        &mut state.session
    } else {
        &mut state.tracker_session
    };
    if let Some(session) = slot.as_ref() {
        return Ok(session.clone());
    }
    let mut options = SessionOptions {
        dht: dht.then(|| DhtSessionConfig {
            persistence: None,
            ..Default::default()
        }),
        persistence: None,
        disable_local_service_discovery: true,
        ipv4_only: true,
        listen: shared
            .options
            .listen_port
            .filter(|_| !listener_in_use)
            .map(|port| ListenerOptions {
                listen_addr: ([0, 0, 0, 0], port).into(),
                ipv4_only: true,
                enable_upnp_port_forwarding: false,
                ..Default::default()
            }),
        ratelimits: librqbit::limits::LimitsConfig {
            upload_bps: shared
                .options
                .upload_limit_bytes_per_sec
                .and_then(|limit| NonZeroU32::new(limit as u32)),
            download_bps: None,
        },
        cancellation_token: Some(shared.stop.child_token()),
        ..Default::default()
    };
    #[cfg(test)]
    if shared.network.offline {
        options.dht = (dht && shared.network.empty_dht).then(|| DhtSessionConfig {
            bootstrap_addrs: Some(shared.network.dht_bootstrap.clone()),
            port: None,
            persistence: None,
        });
        options.disable_trackers = !shared.network.trackers;
        if let Some(listener) = &mut options.listen {
            listener
                .listen_addr
                .set_ip(std::net::Ipv4Addr::LOCALHOST.into());
        }
    }
    #[cfg(not(test))]
    let _ = (&shared.network, &mut options);
    let session = Session::new_with_opts(shared.options.cache_dir.clone(), options)
        .await
        .map_err(|_| TorrentError::Engine)?;
    *slot = Some(session.clone());
    Ok(session)
}

pub(super) async fn open(
    shared: &Arc<Shared>,
    source: TorrentSource,
    host: std::sync::Weak<Host>,
    cancel: CancellationToken,
    started: Instant,
) -> Result<TorrentStream, TorrentError> {
    let metadata_deadline = started + shared.options.metadata_timeout;
    let payload_deadline = started + shared.options.no_peers_timeout;
    let initialization = {
        let mut initializations = lock(&shared.initializations);
        initializations.retain(|_, value| value.strong_count() > 0);
        if let Some(existing) = initializations
            .get(&source.hash)
            .and_then(std::sync::Weak::upgrade)
        {
            existing
        } else {
            let gate = Arc::new(tokio::sync::Mutex::new(()));
            initializations.insert(source.hash.clone(), Arc::downgrade(&gate));
            gate
        }
    };
    let _initialization = timeout_at(metadata_deadline, initialization.lock())
        .await
        .map_err(|_| TorrentError::MetadataTimeout)?;
    let (session, existing) = {
        let mut state = timeout_at(metadata_deadline, shared.state.lock())
            .await
            .map_err(|_| TorrentError::MetadataTimeout)?;
        let existing = state.torrents.get(&source.hash).cloned();
        let session = if let Some(entry) = &existing {
            entry.session.upgrade().ok_or(TorrentError::Cancelled)?
        } else {
            timeout_at(metadata_deadline, session(shared, &mut state, source.dht))
                .await
                .map_err(|_| TorrentError::MetadataTimeout)??
        };
        (session, existing)
    };
    let (entry, index, file_name, size) = if let Some(entry) = existing {
        entry.retry().await?;
        timeout_at(metadata_deadline, entry.torrent.wait_until_initialized())
            .await
            .map_err(|_| TorrentError::MetadataTimeout)?
            .map_err(|_| TorrentError::Engine)?;
        let metadata = entry
            .torrent
            .metadata
            .load_full()
            .ok_or(TorrentError::Engine)?;
        let files: Vec<_> = metadata
            .file_infos
            .iter()
            .map(|file| (file.relative_filename.clone(), file.len))
            .collect();
        let index = choose(&files, source.file_idx)?;
        let mut selected = lock(&entry.selected).clone();
        selected.insert(index);
        *lock(&entry.selected) = selected;
        (entry, index, display_name(&files[index].0), files[index].1)
    } else {
        let mut add_options = AddTorrentOptions {
            list_only: true,
            ..Default::default()
        };
        #[cfg(test)]
        {
            add_options.initial_peers = Some(shared.network.peers.clone());
        }
        #[cfg(not(test))]
        let _ = &mut add_options;
        let response = timeout_at(
            metadata_deadline,
            session.add_torrent(AddTorrent::from_url(source.magnet_uri()), Some(add_options)),
        )
        .await
        .map_err(|_| TorrentError::MetadataTimeout)?
        .map_err(|_| TorrentError::NoPeers)?;
        let AddTorrentResponse::ListOnly(list) = response else {
            return Err(TorrentError::Engine);
        };
        let files: Vec<_> = list
            .info
            .iter_file_details()
            .map(|file| (file.filename.to_pathbuf(), file.len))
            .collect();
        let index = choose(&files, source.file_idx)?;
        let size = files[index].1;
        let cache = {
            let mut state = timeout_at(metadata_deadline, shared.state.lock())
                .await
                .map_err(|_| TorrentError::MetadataTimeout)?;
            make_room(shared, &mut state, size).await?;
            let budget = shared.budget.clone();
            let directory = shared.directory.clone();
            let hash = source.hash.clone();
            tokio::task::spawn_blocking(move || {
                Ok::<_, TorrentError>(Cache::in_directory(directory.child(&hash)?, budget))
            })
            .await
            .map_err(|_| TorrentError::Engine)??
        };
        let torrent = timeout_at(
            metadata_deadline,
            session.add_torrent(
                AddTorrent::from_bytes(list.torrent_bytes),
                Some(AddTorrentOptions {
                    paused: true,
                    only_files: Some(vec![index]),
                    initial_peers: Some(list.seen_peers),
                    storage_factory: Some(Factory(cache.clone()).boxed()),
                    overwrite: true,
                    ..Default::default()
                }),
            ),
        )
        .await
        .map_err(|_| TorrentError::MetadataTimeout)?
        .map_err(|_| TorrentError::Engine)?
        .into_handle()
        .ok_or(TorrentError::Engine)?;
        let entry = Arc::new(Entry::new(torrent, session.clone(), cache, index));
        shared
            .state
            .lock()
            .await
            .torrents
            .insert(source.hash.clone(), entry.clone());
        timeout_at(metadata_deadline, entry.torrent.wait_until_initialized())
            .await
            .map_err(|_| TorrentError::MetadataTimeout)?
            .map_err(|_| TorrentError::Engine)?;
        (entry, index, display_name(&files[index].0), size)
    };
    let mut reader = timeout_at(payload_deadline, entry.reader(index))
        .await
        .map_err(|_| TorrentError::NoPeers)??;
    let mut first = [0; 1];
    let result = timeout_at(payload_deadline, reader.inner.read_exact(&mut first)).await;
    drop(reader);
    entry.monitor(shared.options.no_peers_timeout).await;
    match result {
        Err(_) => return Err(entry.snapshot().error.unwrap_or(TorrentError::NoPeers)),
        Ok(Err(_)) => return Err(entry.snapshot().error.unwrap_or(TorrentError::Engine)),
        Ok(Ok(_)) => {}
    }
    let token = token()?;
    let playback = Arc::new(Playback {
        entry: entry.clone(),
        index,
        size,
        stop: cancel.child_token(),
    });
    lock(&entry.activity).opens += 1;
    lock(&shared.routes).insert(token.clone(), playback.clone());
    Ok(TorrentStream {
        url: format!("http://{}/torrent/{token}", shared.host),
        file_name,
        file_size: size,
        playback,
        token,
        host,
    })
}

async fn make_room(shared: &Shared, state: &mut State, size: u64) -> Result<(), TorrentError> {
    let required = size
        .clamp(BLOCK, 32 * 1024 * 1024)
        .min(shared.options.max_cache_bytes);
    let mut inactive: Vec<_> = state
        .torrents
        .iter()
        .filter_map(|(hash, entry)| {
            let activity = lock(&entry.activity);
            (activity.opens == 0
                && entry.readers.load(Ordering::Acquire) == 0
                && !lock(&shared.initializations)
                    .get(hash)
                    .is_some_and(|gate| gate.strong_count() > 0))
            .then_some((hash.clone(), activity.last_used))
        })
        .collect();
    inactive.sort_by_key(|(_, last)| *last);
    for (hash, _) in inactive {
        match check_room(shared, required).await {
            Ok(()) => break,
            Err(TorrentError::Disk(DiskError::Full)) => {}
            Err(error) => return Err(error),
        }
        if let Some(entry) = state.torrents.remove(&hash) {
            session_delete(&entry).await?;
            tokio::task::spawn_blocking(move || entry.cache.retire())
                .await
                .map_err(|_| TorrentError::Engine)??;
        }
    }
    check_room(shared, required).await
}

async fn check_room(shared: &Shared, required: u64) -> Result<(), TorrentError> {
    let budget = shared.budget.clone();
    let path = shared.options.cache_dir.clone();
    tokio::task::spawn_blocking(move || {
        let budget = lock(&budget);
        if required > budget.limit.saturating_sub(budget.used) {
            return Err(TorrentError::Disk(DiskError::Full));
        }
        budget.available(&path, required)
    })
    .await
    .map_err(|_| TorrentError::Engine)?
}

async fn session_delete(entry: &Entry) -> Result<(), TorrentError> {
    entry
        .session
        .upgrade()
        .ok_or(TorrentError::Cancelled)?
        .delete(
            librqbit::api::TorrentIdOrHash::Id(entry.torrent.id()),
            false,
        )
        .await
        .map_err(|_| TorrentError::Engine)
}

fn choose(
    files: &[(std::path::PathBuf, u64)],
    index: Option<usize>,
) -> Result<usize, TorrentError> {
    if let Some(index) = index {
        return files
            .get(index)
            .filter(|(_, size)| *size > 0)
            .map(|_| index)
            .ok_or(TorrentError::InvalidSource);
    }
    files
        .iter()
        .enumerate()
        .filter(|(_, (path, size))| {
            *size > 0
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        matches!(
                            ext.to_ascii_lowercase().as_str(),
                            "mkv" | "mp4" | "avi" | "mov" | "webm" | "m4v" | "ts"
                        )
                    })
        })
        .max_by_key(|(_, (_, size))| *size)
        .map(|(index, _)| index)
        .ok_or(TorrentError::NoVideoFile)
}
fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
fn token() -> Result<String, TorrentError> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| TorrentError::Engine)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
