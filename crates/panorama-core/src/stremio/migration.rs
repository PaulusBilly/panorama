use std::{cell::RefCell, collections::BTreeMap};

use chrono::{DateTime, Utc};
use futures::Future;
use http::Request;
use serde::{Deserialize, Serialize};
use stremio_core::{
    models::{ctx::Ctx, streaming_server::StreamingServer},
    runtime::{ConditionalSend, Env, EnvError, EnvFuture, EnvFutureExt, TryEnvFuture},
};

use super::env::PanoramaEnv;
use crate::store::Key;

tokio::task_local! {
    static WRITES: RefCell<BTreeMap<String, Option<Vec<u8>>>>;
}

struct MigrationEnv;

pub(super) fn migrate() -> TryEnvFuture<()> {
    async {
        let state = PanoramaEnv::state()?;
        let writes = WRITES
            .scope(RefCell::new(BTreeMap::new()), async {
                MigrationEnv::migrate_storage_schema().await?;
                WRITES.with(|writes| {
                    writes
                        .take()
                        .into_iter()
                        .map(|(key, value)| {
                            Key::core(&key)
                                .map(|key| (key, value))
                                .map_err(|_| EnvError::StorageWriteError("invalid bucket".into()))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
            })
            .await?;
        if !writes.is_empty() {
            #[cfg(test)]
            let hook = state.before_migration_commit.lock().unwrap().take();
            state
                .blocking(move |store| {
                    #[cfg(test)]
                    let finished = hook.map(|hook| hook());
                    let result = store.set_many(&writes);
                    #[cfg(test)]
                    if let Some(finished) = finished {
                        let _ = finished.send(());
                    }
                    result
                })
                .await?;
        }
        Ok(())
    }
    .boxed_env()
}

impl Env for MigrationEnv {
    fn get_storage<T: for<'de> Deserialize<'de> + ConditionalSend + 'static>(
        key: &str,
    ) -> TryEnvFuture<Option<T>> {
        let staged = WRITES.with(|writes| writes.borrow().get(key).cloned());
        match staged {
            Some(bytes) => async move {
                bytes
                    .map(|bytes| serde_json::from_slice(&bytes))
                    .transpose()
                    .map_err(|_| EnvError::StorageReadError("invalid stored JSON".into()))
            }
            .boxed_env(),
            None => PanoramaEnv::get_storage(key),
        }
    }

    fn set_storage<T: Serialize>(key: &str, value: Option<&T>) -> TryEnvFuture<()> {
        let bytes = value.map(serde_json::to_vec).transpose();
        let key = key.to_owned();
        async move {
            let bytes = bytes
                .map_err(|_| EnvError::StorageWriteError("storage serialization failed".into()))?;
            WRITES.with(|writes| writes.borrow_mut().insert(key, bytes));
            Ok(())
        }
        .boxed_env()
    }

    fn fetch<
        IN: Serialize + ConditionalSend + 'static,
        OUT: for<'de> Deserialize<'de> + ConditionalSend + 'static,
    >(
        request: Request<IN>,
    ) -> TryEnvFuture<OUT> {
        PanoramaEnv::fetch(request)
    }

    fn exec_concurrent<F: Future<Output = ()> + ConditionalSend + 'static>(future: F) {
        PanoramaEnv::exec_concurrent(future);
    }

    fn exec_sequential<F: Future<Output = ()> + ConditionalSend + 'static>(future: F) {
        PanoramaEnv::exec_sequential(future);
    }

    fn now() -> DateTime<Utc> {
        PanoramaEnv::now()
    }

    fn flush_analytics() -> EnvFuture<'static, ()> {
        PanoramaEnv::flush_analytics()
    }

    fn analytics_context(ctx: &Ctx, server: &StreamingServer, path: &str) -> serde_json::Value {
        PanoramaEnv::analytics_context(ctx, server, path)
    }

    #[cfg(debug_assertions)]
    fn log(_: String) {}
}
