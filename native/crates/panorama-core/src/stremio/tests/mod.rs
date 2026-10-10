mod addons;
mod cancellation;
mod fetch;
mod lifecycle;
mod mock;
mod progress;
mod session;
mod storage;
mod tls;

use std::sync::{Arc, MutexGuard};

use tokio::runtime::Runtime;
use url::Url;

use crate::{
    store::Store,
    stremio::env::{EnvConfig, PanoramaEnv, State, TEST_MUTEX},
};

struct TestEnv {
    runtime: Runtime,
    state: Arc<State>,
    guard: MutexGuard<'static, ()>,
}

impl TestEnv {
    fn new(store: Store, api_base: Option<Url>) -> Self {
        let guard = TEST_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        PanoramaEnv::swap_for_test(
            EnvConfig {
                store: Arc::new(store),
                api_base,
                runtime: runtime.handle().clone(),
            },
            &guard,
        );
        let state = PanoramaEnv::state().unwrap();
        Self {
            guard,
            runtime,
            state,
        }
    }

    fn memory(api_base: Option<Url>) -> Self {
        Self::new(Store::open_in_memory().unwrap(), api_base)
    }

    fn trust_tls(&mut self, servers: &[&tls::TlsMock]) {
        self.idle();
        let mut builder = super::fetch::client_builder(self.state.api_base.as_ref()).unwrap();
        for server in servers {
            builder = builder.add_root_certificate(server.certificate.clone());
        }
        PanoramaEnv::swap_client_for_test(builder.build().unwrap(), &self.guard);
        self.state = PanoramaEnv::state().unwrap();
    }

    fn idle(&self) {
        self.runtime.block_on(async {
            let _lease = self.state.session_gate.lock().await;
            self.state.cancel_concurrent().await;
            self.state.drain().await.unwrap();
        });
    }

    fn replace(&mut self, store: Store, api_base: Option<Url>) {
        self.idle();
        PanoramaEnv::swap_for_test(
            EnvConfig {
                store: Arc::new(store),
                api_base,
                runtime: self.runtime.handle().clone(),
            },
            &self.guard,
        );
        self.state = PanoramaEnv::state().unwrap();
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        self.idle();
        PanoramaEnv::swap_for_test(
            EnvConfig {
                store: Arc::new(Store::open_in_memory().unwrap()),
                api_base: None,
                runtime: self.runtime.handle().clone(),
            },
            &self.guard,
        );
    }
}
