use super::{
    CancellationToken, TorrentError, TorrentOptions, TorrentSource, TorrentStream,
    cache::BLOCK,
    lock,
    opening::open,
    runtime::{Host, Network, OwnerThread, Shared},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::time::Instant;

/// Lazy, reusable, UI-free torrent playback engine. Call `shutdown` on app exit.
pub struct TorrentEngine {
    options: TorrentOptions,
    network: Network,
    host: tokio::sync::Mutex<Option<Arc<Host>>>,
    stopped: AtomicBool,
    shutdown_signal: CancellationToken,
    starts: Mutex<Vec<OwnerThread>>,
}

impl TorrentEngine {
    /// Construct without I/O or tasks. Invalid options are rejected by `open`.
    pub fn new(options: TorrentOptions) -> Self {
        Self {
            options,
            network: Network::default(),
            host: tokio::sync::Mutex::new(None),
            stopped: AtomicBool::new(false),
            shutdown_signal: CancellationToken::new(),
            starts: Mutex::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub(super) fn offline(options: TorrentOptions, peers: Vec<std::net::SocketAddr>) -> Self {
        let mut engine = Self::new(options);
        engine.network = Network {
            offline: true,
            peers,
            empty_dht: false,
            dht_bootstrap: vec![],
            trackers: false,
        };
        engine
    }

    #[cfg(test)]
    pub(super) fn offline_dht(options: TorrentOptions, bootstrap: std::net::SocketAddr) -> Self {
        let mut engine = Self::offline(options, vec![]);
        engine.network.empty_dht = true;
        engine.network.dht_bootstrap = vec![bootstrap.to_string()];
        engine
    }

    #[cfg(test)]
    pub(super) fn trackers_for_test(&mut self) {
        self.network.trackers = true;
    }

    #[cfg(test)]
    pub(super) async fn host_for_test(&self) -> Arc<Host> {
        self.host.lock().await.as_ref().unwrap().clone()
    }

    /// Resolve metadata, select a file, and wait for its first verified payload.
    /// Each stage observes cancellation and a configured finite deadline.
    /// Cancelling `cancel` also invalidates the returned stream and its readers.
    pub async fn open(
        &self,
        source: TorrentSource,
        cancel: CancellationToken,
    ) -> Result<TorrentStream, TorrentError> {
        validate(&self.options)?;
        if self.stopped.load(Ordering::Acquire) || cancel.is_cancelled() {
            return Err(TorrentError::Cancelled);
        }
        let started = Instant::now();
        let acquire = async {
            let mut slot = self.host.lock().await;
            if self.stopped.load(Ordering::Acquire) {
                return Err(TorrentError::Cancelled);
            }
            loop {
                if let Some(old) = slot.as_ref()
                    && old.shared.stop.is_cancelled()
                {
                    old.shutdown().await?;
                    slot.take();
                }
                if slot.is_none() {
                    *slot = Some(
                        Host::start(self.options.clone(), self.network.clone(), &self.starts)
                            .await?,
                    );
                }
                let host = slot.as_ref().ok_or(TorrentError::Engine)?.clone();
                let admitted = {
                    let accepting = lock(&host.shared.accepting);
                    if *accepting && !host.shared.stop.is_cancelled() {
                        host.shared.operations.fetch_add(1, Ordering::AcqRel);
                        true
                    } else {
                        false
                    }
                };
                if admitted {
                    let operation = Operation(host.shared.clone());
                    break Ok::<_, TorrentError>((host, operation));
                }
                host.shared.stop.cancel();
            }
        };
        let (host, operation) = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TorrentError::Cancelled),
            _ = self.shutdown_signal.cancelled() => return Err(TorrentError::Cancelled),
            _ = tokio::time::sleep_until(started + self.options.metadata_timeout) => return Err(TorrentError::MetadataTimeout),
            result = acquire => result?,
        };
        let shared = host.shared.clone();
        let weak_host = Arc::downgrade(&host);
        let task = host.handle.spawn(async move {
            let _operation = operation;
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(TorrentError::Cancelled),
                _ = shared.stop.cancelled() => Err(TorrentError::Cancelled),
                result = open(&shared, source, weak_host, cancel.clone(), started) => result,
            };
            result
        });
        let _abort = Abort(task.abort_handle());
        task.await.map_err(|_| TorrentError::Engine)?
    }

    /// Reject future opens, drain/abort and join HTTP tasks, stop the library,
    /// dispose its private runtime, and remove the engine-owned cache directory.
    pub async fn shutdown(&self) -> Result<(), TorrentError> {
        self.stopped.store(true, Ordering::Release);
        self.shutdown_signal.cancel();
        let host = self.host.lock().await;
        let result = if let Some(host) = host.as_ref() {
            host.shutdown().await
        } else {
            Ok(())
        };
        let threads: Vec<_> = lock(&self.starts)
            .drain(..)
            .filter_map(|thread| lock(&thread).take())
            .collect();
        if !threads.is_empty() {
            tokio::task::spawn_blocking(move || {
                for thread in threads {
                    let _ = thread.join().map_err(|_| TorrentError::Engine)?;
                }
                Ok::<_, TorrentError>(())
            })
            .await
            .map_err(|_| TorrentError::Engine)??;
        }
        result
    }
}

impl Drop for TorrentEngine {
    fn drop(&mut self) {
        self.shutdown_signal.cancel();
        if let Some(host) = self.host.get_mut().as_ref() {
            host.shared.stop.cancel();
        }
    }
}

struct Abort(tokio::task::AbortHandle);
impl Drop for Abort {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Operation(Arc<Shared>);
impl Drop for Operation {
    fn drop(&mut self) {
        self.0.operations.fetch_sub(1, Ordering::AcqRel);
        if let Ok(mut state) = self.0.state.try_lock()
            && lock(&self.0.routes).is_empty()
        {
            state.idle_since = Instant::now();
        }
    }
}

fn validate(options: &TorrentOptions) -> Result<(), TorrentError> {
    if options.max_cache_bytes < BLOCK
        || options.cache_dir.as_os_str().is_empty()
        || options.idle_stop_after.is_zero()
        || options.metadata_timeout.is_zero()
        || options.no_peers_timeout.is_zero()
        || options
            .upload_limit_bytes_per_sec
            .is_some_and(|value| value == 0 || value > u64::from(u32::MAX))
    {
        return Err(TorrentError::InvalidSource);
    }
    Ok(())
}
