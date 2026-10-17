use super::{
    TorrentError, TorrentOptions,
    cache::Budget,
    directory::OwnedDir,
    lock, server,
    stream::{Entry, Playback},
};
use librqbit::Session;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    runtime::{Builder, Handle},
    sync::oneshot,
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

thread_local! {
    static QUIET: RefCell<Option<tracing::dispatcher::DefaultGuard>> = const { RefCell::new(None) };
}

#[derive(Clone, Default)]
pub(super) struct Network {
    #[cfg(test)]
    pub offline: bool,
    #[cfg(test)]
    pub peers: Vec<std::net::SocketAddr>,
    #[cfg(test)]
    pub empty_dht: bool,
    #[cfg(test)]
    pub dht_bootstrap: Vec<String>,
    #[cfg(test)]
    pub trackers: bool,
}

pub(super) struct State {
    pub session: Option<Arc<Session>>,
    pub tracker_session: Option<Arc<Session>>,
    pub torrents: HashMap<String, Arc<Entry>>,
    pub idle_since: Instant,
}

pub(super) struct Shared {
    pub options: TorrentOptions,
    pub network: Network,
    pub stop: CancellationToken,
    pub state: tokio::sync::Mutex<State>,
    pub routes: Mutex<HashMap<String, Arc<Playback>>>,
    pub budget: Arc<Mutex<Budget>>,
    pub directory: Arc<OwnedDir>,
    pub initializations: Mutex<HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
    pub host: String,
    pub operations: AtomicUsize,
    pub accepting: Mutex<bool>,
}

impl Shared {
    pub async fn release(&self, token: &str) {
        let playback = lock(&self.routes).remove(token);
        if let Some(playback) = playback {
            playback.stop.cancel();
            {
                let mut activity = lock(&playback.entry.activity);
                activity.opens = activity.opens.saturating_sub(1);
                activity.last_used = Instant::now();
            }
            let _guard = playback.entry.control.lock().await;
            let inactive = lock(&playback.entry.activity).opens == 0
                && self.operations.load(Ordering::Acquire) == 0;
            if inactive {
                playback.entry.snapshot();
                if let Some(session) = playback.entry.session.upgrade() {
                    let _ = session.pause(&playback.entry.torrent).await;
                }
            }
            drop(_guard);
            if inactive {
                let _ = tokio::time::timeout(Duration::from_millis(500), async {
                    while playback.entry.readers.load(Ordering::Acquire) > 0 {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await;
            }
            if let Ok(mut state) = self.state.try_lock()
                && lock(&self.routes).is_empty()
            {
                state.idle_since = Instant::now();
            }
        }
    }
}

pub(super) type OwnerThread = Arc<Mutex<Option<std::thread::JoinHandle<Result<(), TorrentError>>>>>;

pub(super) struct Host {
    pub handle: Handle,
    pub shared: Arc<Shared>,
    pub completion: tokio::sync::Mutex<Option<JoinHandle<Result<(), TorrentError>>>>,
    thread: OwnerThread,
    outcome: Mutex<Option<Result<(), TorrentError>>>,
}

impl Host {
    pub async fn start(
        options: TorrentOptions,
        network: Network,
        starts: &Mutex<Vec<OwnerThread>>,
    ) -> Result<Arc<Self>, TorrentError> {
        let (sender, receiver) = oneshot::channel();
        let mut startup = Startup(Some(CancellationToken::new()));
        let stop = startup.0.as_ref().ok_or(TorrentError::Engine)?.clone();
        let thread = std::thread::Builder::new()
            .name("panorama-torrent".into())
            .spawn(move || {
                let mut sender = Some(sender);
                let quiet = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
                let _guard = tracing::dispatcher::set_default(&quiet);
                let runtime = Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .on_thread_start(|| {
                        let dispatch =
                            tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
                        QUIET.with(|quiet| {
                            *quiet.borrow_mut() = Some(tracing::dispatcher::set_default(&dispatch))
                        });
                    })
                    .build()
                    .map_err(|_| TorrentError::Engine)?;
                let result = runtime.block_on(async {
                    let directory = Arc::new(OwnedDir::create(&options.cache_dir)?);
                    let mut options = options;
                    options.cache_dir = directory.path.clone();
                    let setup = async {
                        let budget = Budget::new(&options.cache_dir, options.max_cache_bytes)?;
                        let listener =
                            tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                                .await
                                .map_err(|_| TorrentError::Engine)?;
                        let address = listener.local_addr().map_err(|_| TorrentError::Engine)?;
                        let shared = Arc::new(Shared {
                            options: options.clone(),
                            network,
                            stop,
                            state: tokio::sync::Mutex::new(State {
                                session: None,
                                tracker_session: None,
                                torrents: HashMap::new(),
                                idle_since: Instant::now(),
                            }),
                            routes: Mutex::new(HashMap::new()),
                            budget,
                            directory: directory.clone(),
                            initializations: Mutex::new(HashMap::new()),
                            host: address.to_string(),
                            operations: AtomicUsize::new(0),
                            accepting: Mutex::new(true),
                        });
                        if sender
                            .take()
                            .ok_or(TorrentError::Engine)?
                            .send(Ok((runtime.handle().clone(), shared.clone())))
                            .is_err()
                        {
                            shared.stop.cancel();
                        }
                        run(listener, shared).await
                    }
                    .await;
                    let removed = directory.remove();
                    setup.and(removed)
                });
                if let Some(sender) = sender {
                    let error = result
                        .as_ref()
                        .err()
                        .copied()
                        .unwrap_or(TorrentError::Engine);
                    let _ = sender.send(Err(error));
                }
                runtime.shutdown_timeout(Duration::from_secs(2));
                result
            })
            .map_err(|_| TorrentError::Engine)?;
        let thread = Arc::new(Mutex::new(Some(thread)));
        {
            let mut starts = lock(starts);
            starts.retain(|thread| lock(thread).is_some());
            starts.push(thread.clone());
        }
        let (handle, shared) = receiver.await.map_err(|_| TorrentError::Engine)??;
        startup.0.take();
        Ok(Arc::new(Self {
            handle,
            shared,
            completion: tokio::sync::Mutex::new(None),
            thread,
            outcome: Mutex::new(None),
        }))
    }

    pub async fn shutdown(&self) -> Result<(), TorrentError> {
        self.shared.stop.cancel();
        let mut completion = self.completion.lock().await;
        if completion.is_none()
            && let Some(thread) = lock(&self.thread).take()
        {
            *completion = Some(tokio::task::spawn_blocking(move || {
                thread.join().unwrap_or(Err(TorrentError::Engine))
            }));
        }
        if let Some(task) = completion.as_mut() {
            let result = task.await.unwrap_or(Err(TorrentError::Engine));
            completion.take();
            *lock(&self.outcome) = Some(result);
        }
        lock(&self.outcome).unwrap_or(Ok(()))
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.shared.stop.cancel();
    }
}

struct Startup(Option<CancellationToken>);
impl Drop for Startup {
    fn drop(&mut self) {
        if let Some(stop) = &self.0 {
            stop.cancel();
        }
    }
}

async fn run(listener: tokio::net::TcpListener, shared: Arc<Shared>) -> Result<(), TorrentError> {
    let mut server = tokio::spawn(server::accept(listener, shared.clone()));
    let mut interval = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            biased;
            _ = shared.stop.cancelled() => break,
            _ = interval.tick() => {
                let expired: Vec<_> = lock(&shared.routes).iter().filter(|(_, stream)| stream.stop.is_cancelled()).map(|(token, _)| token.clone()).collect();
                for token in expired { shared.release(&token).await; }
                let entries: Vec<_> = lock(&shared.routes).values().map(|playback| playback.entry.clone()).collect();
                for entry in entries { entry.monitor(shared.options.no_peers_timeout).await; }
                let entries = shared.state.try_lock().ok().map(|state|
                    state.torrents.values().cloned().collect::<Vec<_>>()).unwrap_or_default();
                for entry in entries { entry.monitor(shared.options.no_peers_timeout).await; }
                if let Ok(state) = shared.state.try_lock() {
                    let mut accepting = lock(&shared.accepting);
                    if shared.operations.load(Ordering::Acquire) == 0 && lock(&shared.routes).is_empty()
                        && state.idle_since.elapsed() >= shared.options.idle_stop_after {
                        *accepting = false;
                        shared.stop.cancel();
                    }
                }
            }
            _ = &mut server => { shared.stop.cancel(); break; }
        }
    }
    for playback in lock(&shared.routes).values() {
        playback.stop.cancel();
    }
    if !server.is_finished() {
        let _ = (&mut server).await;
    }
    let mut state = shared.state.lock().await;
    lock(&shared.routes).clear();
    let sessions: Vec<_> = [state.session.take(), state.tracker_session.take()]
        .into_iter()
        .flatten()
        .collect();
    futures::future::join_all(
        sessions
            .iter()
            .map(|session| tokio::time::timeout(Duration::from_secs(3), session.stop())),
    )
    .await;
    for entry in state.torrents.values() {
        entry.cache.retire()?;
    }
    state.torrents.clear();
    Ok(())
}
