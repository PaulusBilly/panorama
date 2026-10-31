//! Static core Env configuration and ordered, off-thread SQLite persistence.

use std::{
    fmt,
    sync::{
        Arc, Mutex, OnceLock, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::{DateTime, Utc};
use futures::Future;
use http::Request;
use serde::{Deserialize, Serialize};
use stremio_core::{
    models::{ctx::Ctx, streaming_server::StreamingServer},
    runtime::{ConditionalSend, Env, EnvError, EnvFuture, EnvFutureExt, TryEnvFuture},
};
use tokio::{
    runtime::Handle,
    sync::{OwnedMutexGuard, mpsc, oneshot},
    task::JoinHandle,
};
use tracing::{instrument::WithSubscriber, subscriber::NoSubscriber};
use url::Url;

use super::{CoreError, CoreErrorKind, fetch};
use crate::store::{Key, Store, StoreError};

#[cfg(test)]
type MigrationCommitHook = Box<dyn FnOnce() -> oneshot::Sender<()> + Send>;

/// Resources used by every core Env call in this process.
pub struct EnvConfig {
    /// Shared SQLite store; all Env access runs on blocking workers.
    pub store: Arc<Store>,
    /// Optional HTTPS origin, or literal loopback HTTP origin for local tests.
    pub api_base: Option<Url>,
    /// Runtime that owns core workers and blocking tasks; keep it alive.
    pub runtime: Handle,
}

impl fmt::Debug for EnvConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EnvConfig { .. }")
    }
}

pub(super) struct State {
    pub(super) client: reqwest::Client,
    pub(super) api_base: Option<Url>,
    pub(super) store: Arc<Store>,
    pub(super) runtime: Handle,
    sequential: mpsc::UnboundedSender<EnvFuture<'static, ()>>,
    concurrent: Mutex<Vec<JoinHandle<()>>>,
    storage_failed: AtomicBool,
    pub(super) session_gate: Arc<tokio::sync::Mutex<()>>,
    pub(super) session_lease: Mutex<Weak<OwnedMutexGuard<()>>>,
    #[cfg(test)]
    pub(super) store_threads: Mutex<Vec<std::thread::ThreadId>>,
    #[cfg(test)]
    pub(super) before_sequential: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(super) before_migration_commit: Mutex<Option<MigrationCommitHook>>,
}

static STATE: OnceLock<RwLock<Arc<State>>> = OnceLock::new();

/// Stremio's process-wide environment, installed exactly once in production.
#[derive(Debug)]
pub struct PanoramaEnv;

impl PanoramaEnv {
    /// Installs runtime, HTTP policy and store; a second install returns an error.
    pub fn install(config: EnvConfig) -> Result<(), CoreError> {
        if STATE.get().is_some() {
            return Err(CoreErrorKind::AlreadyInstalled.into());
        }
        STATE
            .set(RwLock::new(State::new(config)?))
            .map_err(|_| CoreErrorKind::AlreadyInstalled.into())
    }

    pub(super) fn state() -> Result<Arc<State>, EnvError> {
        STATE
            .get()
            .map(|state| {
                Arc::clone(
                    &state
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()),
                )
            })
            .ok_or(EnvError::StorageUnavailable)
    }

    #[cfg(test)]
    pub(super) fn swap_for_test(config: EnvConfig, _: &std::sync::MutexGuard<'_, ()>) {
        let state = State::new(config).expect("test environment");
        let slot = STATE.get_or_init(|| RwLock::new(Arc::clone(&state)));
        *slot
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
    }

    #[cfg(test)]
    pub(super) fn swap_client_for_test(client: reqwest::Client, _: &std::sync::MutexGuard<'_, ()>) {
        let current = Self::state().unwrap();
        let mut state = State::new(EnvConfig {
            store: Arc::clone(&current.store),
            api_base: current.api_base.clone(),
            runtime: current.runtime.clone(),
        })
        .unwrap();
        Arc::get_mut(&mut state).unwrap().client = client;
        *STATE.get().unwrap().write().unwrap() = state;
    }
}

impl State {
    fn new(config: EnvConfig) -> Result<Arc<Self>, CoreError> {
        let client = fetch::client(config.api_base.as_ref())?;
        let (sequential, mut queue) = mpsc::unbounded_channel::<EnvFuture<'static, ()>>();
        config.runtime.spawn(
            async move {
                while let Some(future) = queue.recv().await {
                    future.await;
                }
            }
            .with_subscriber(NoSubscriber::default()),
        );
        Ok(Arc::new(Self {
            client,
            api_base: config.api_base,
            store: config.store,
            runtime: config.runtime,
            sequential,
            concurrent: Mutex::new(vec![]),
            storage_failed: AtomicBool::new(false),
            session_gate: Arc::new(tokio::sync::Mutex::new(())),
            session_lease: Mutex::new(Weak::new()),
            #[cfg(test)]
            store_threads: Mutex::new(vec![]),
            #[cfg(test)]
            before_sequential: Mutex::new(None),
            #[cfg(test)]
            before_migration_commit: Mutex::new(None),
        }))
    }

    pub(super) async fn blocking<T: Send + 'static>(
        self: &Arc<Self>,
        operation: impl FnOnce(&Store) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, EnvError> {
        let state = Arc::clone(self);
        let lease = self
            .session_lease
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .upgrade();
        let result = self
            .runtime
            .spawn_blocking(move || {
                let _lease = lease;
                #[cfg(test)]
                state
                    .store_threads
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(std::thread::current().id());
                operation(&state.store)
            })
            .await;
        match result {
            Ok(Ok(value)) => Ok(value),
            _ => Err(EnvError::StorageUnavailable),
        }
    }

    pub(super) async fn drain(&self) -> Result<(), CoreError> {
        let (done, received) = oneshot::channel();
        self.sequential
            .send(
                async move {
                    let _ = done.send(());
                }
                .boxed_env(),
            )
            .map_err(|_| CoreError::from(CoreErrorKind::Storage))?;
        received
            .await
            .map_err(|_| CoreError::from(CoreErrorKind::Storage))?;
        if self.storage_failed.swap(false, Ordering::AcqRel) {
            return Err(CoreErrorKind::Storage.into());
        }
        Ok(())
    }

    pub(super) fn take_concurrent(&self) -> Vec<JoinHandle<()>> {
        std::mem::take(
            &mut *self
                .concurrent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    pub(super) async fn cancel_concurrent(&self) {
        loop {
            let tasks = self.take_concurrent();
            if tasks.is_empty() {
                break;
            }
            for task in &tasks {
                task.abort();
            }
            for task in tasks {
                let _ = task.await;
            }
        }
    }

    pub(super) async fn finish_concurrent(&self) {
        loop {
            let tasks = self.take_concurrent();
            if tasks.is_empty() {
                break;
            }
            for task in tasks {
                let _ = task.await;
            }
        }
    }
}

impl Env for PanoramaEnv {
    fn migrate_storage_schema() -> TryEnvFuture<()> {
        super::migration::migrate()
    }

    fn fetch<
        IN: Serialize + ConditionalSend + 'static,
        OUT: for<'de> Deserialize<'de> + ConditionalSend + 'static,
    >(
        request: Request<IN>,
    ) -> TryEnvFuture<OUT> {
        let state = Self::state();
        async move { fetch::fetch(state?, request).await }
            .with_subscriber(NoSubscriber::default())
            .boxed_env()
    }

    fn get_storage<T: for<'de> Deserialize<'de> + ConditionalSend + 'static>(
        key: &str,
    ) -> TryEnvFuture<Option<T>> {
        let state = Self::state();
        let key = Key::core(key);
        async move {
            let bytes = state?.blocking(move |store| store.get(&key?)).await?;
            bytes
                .map(|bytes| serde_json::from_slice(&bytes))
                .transpose()
                .map_err(|_| EnvError::StorageReadError("invalid stored JSON".into()))
        }
        .boxed_env()
    }

    fn set_storage<T: Serialize>(key: &str, value: Option<&T>) -> TryEnvFuture<()> {
        let state = Self::state();
        let key = Key::core(key);
        let bytes = value.map(serde_json::to_vec).transpose();
        async move {
            let state = state?;
            let result = async {
                let key = key.map_err(|_| EnvError::StorageWriteError("invalid bucket".into()))?;
                let bytes = bytes.map_err(|_| {
                    EnvError::StorageWriteError("storage serialization failed".into())
                })?;
                state
                    .blocking(move |store| match bytes {
                        Some(bytes) => store.set(&key, &bytes),
                        None => store.remove(&key),
                    })
                    .await
            }
            .await;
            if result.is_err() {
                state.storage_failed.store(true, Ordering::Release);
            }
            result
        }
        .boxed_env()
    }

    fn exec_concurrent<F: Future<Output = ()> + ConditionalSend + 'static>(future: F) {
        if let Ok(state) = Self::state() {
            let mut tasks = state.concurrent.lock().unwrap_or_else(|p| p.into_inner());
            tasks.retain(|task| !task.is_finished());
            tasks.push(
                state
                    .runtime
                    .spawn(future.with_subscriber(NoSubscriber::default())),
            );
        }
    }

    fn exec_sequential<F: Future<Output = ()> + ConditionalSend + 'static>(future: F) {
        #[cfg(test)]
        if let Ok(state) = Self::state() {
            let hook = state.before_sequential.lock().unwrap().clone();
            if let Some(hook) = hook {
                hook();
            }
        }
        if let Ok(state) = Self::state()
            && state.sequential.send(future.boxed_env()).is_err()
        {
            state.storage_failed.store(true, Ordering::Release);
        }
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    fn flush_analytics() -> EnvFuture<'static, ()> {
        async {}.boxed_env()
    }

    fn analytics_context(_: &Ctx, _: &StreamingServer, _: &str) -> serde_json::Value {
        serde_json::Value::Null
    }

    #[cfg(debug_assertions)]
    fn log(_: String) {}
}

#[cfg(test)]
pub(super) static TEST_MUTEX: Mutex<()> = Mutex::new(());
