use chrono::{DateTime, Utc};
use futures::{Future, StreamExt};
use http::{Method, Request};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use stremio_core::constants::API_URL;
use stremio_core::models::ctx::Ctx;
use stremio_core::models::streaming_server::StreamingServer;
use stremio_core::runtime::msg::{Action, ActionCtx, Event};
use stremio_core::runtime::{
    ConditionalSend, Env, EnvError, EnvFuture, EnvFutureExt, Runtime, RuntimeAction, RuntimeEvent,
    TryEnvFuture,
};
use stremio_core::types::addon::{
    Descriptor, ManifestResource, ResourcePath, ResourceRequest, ResourceResponse,
};
use stremio_core::types::api::AuthRequest;
use stremio_core::types::events::DismissedEventsBucket;
use stremio_core::types::library::LibraryBucket;
use stremio_core::types::notifications::NotificationsBucket;
use stremio_core::types::profile::Profile;
use stremio_core::types::search_history::SearchHistoryBucket;
use stremio_core::types::server_urls::ServerUrlsBucket;
use stremio_core::types::streams::StreamsBucket;
use tokio::sync::mpsc;
use url::{Host, Url};

const BODY_LIMIT: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

struct State {
    client: reqwest::Client,
    api_base: Option<Url>,
    storage: Mutex<HashMap<String, Vec<u8>>>,
    sequential: mpsc::UnboundedSender<EnvFuture<'static, ()>>,
}

static STATE: OnceLock<State> = OnceLock::new();

pub struct NativeEnv;

impl NativeEnv {
    pub fn initialize(api_base: Option<Url>) -> Result<(), &'static str> {
        if let Some(base) = &api_base {
            let loopback = match base.host() {
                Some(Host::Ipv4(ip)) => ip.is_loopback(),
                Some(Host::Ipv6(ip)) => ip.is_loopback(),
                _ => false,
            };
            if (base.scheme() != "https" && !(base.scheme() == "http" && loopback))
                || !base.username().is_empty()
                || base.password().is_some()
                || base.path() != "/"
                || base.query().is_some()
                || base.fragment().is_some()
            {
                return Err("API override must be an HTTPS origin or a loopback HTTP origin");
            }
        }
        let override_origin = api_base.as_ref().map(Url::origin);
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(api_base.is_none())
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 10 {
                    return attempt.error("redirect limit reached");
                }
                if attempt.url().scheme() != "https" {
                    return attempt.error("HTTPS is required for redirects");
                }
                if let Some(first) = attempt.previous().first()
                    && (first.origin() == API_URL.origin()
                        || override_origin.as_ref() == Some(&first.origin()))
                    && attempt.url().origin() != first.origin()
                {
                    return attempt.error("API redirects must keep the same origin");
                }
                attempt.follow()
            }))
            .timeout(TIMEOUT)
            .build()
            .map_err(|_| "HTTP client initialization failed")?;
        let (sequential, mut queue) = mpsc::unbounded_channel::<EnvFuture<'static, ()>>();
        STATE
            .set(State {
                client,
                api_base,
                storage: Mutex::new(HashMap::new()),
                sequential,
            })
            .map_err(|_| "Env is already initialized")?;
        tokio::spawn(async move {
            while let Some(future) = queue.recv().await {
                future.await;
            }
        });
        Ok(())
    }

    fn state() -> &'static State {
        STATE.get().expect("Env must be initialized")
    }
}

impl Env for NativeEnv {
    fn fetch<
        IN: Serialize + ConditionalSend + 'static,
        OUT: for<'de> Deserialize<'de> + ConditionalSend + 'static,
    >(
        request: Request<IN>,
    ) -> TryEnvFuture<OUT> {
        async move {
            let state = Self::state();
            let (parts, body) = request.into_parts();
            let mut url = Url::parse(&parts.uri.to_string())
                .map_err(|_| EnvError::Fetch("invalid request URL".into()))?;
            let core_api = url.origin() == API_URL.origin();
            let mut loopback_override = false;
            if core_api && let Some(base) = &state.api_base {
                loopback_override = base.scheme() == "http";
                url.set_scheme(base.scheme())
                    .map_err(|_| EnvError::Fetch("invalid override scheme".into()))?;
                url.set_host(base.host_str())
                    .map_err(|_| EnvError::Fetch("invalid override host".into()))?;
                url.set_port(base.port())
                    .map_err(|_| EnvError::Fetch("invalid override port".into()))?;
            }
            if url.scheme() != "https" && !loopback_override {
                return Err(EnvError::Fetch("HTTPS is required".into()));
            }
            let mut builder = state
                .client
                .request(parts.method.clone(), url)
                .headers(parts.headers);
            if parts.method != Method::GET && parts.method != Method::HEAD {
                let bytes = serde_json::to_vec(&body)
                    .map_err(|_| EnvError::Serde("request serialization failed".into()))?;
                builder = builder
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(bytes);
            }
            let mut response = builder
                .send()
                .await
                .map_err(|_| EnvError::Fetch("request failed or timed out".into()))?;
            if !response.status().is_success() {
                return Err(EnvError::Fetch(format!(
                    "HTTP status {}",
                    response.status().as_u16()
                )));
            }
            if response
                .content_length()
                .is_some_and(|length| length > BODY_LIMIT as u64)
            {
                return Err(EnvError::Fetch("response exceeds 8 MiB".into()));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| EnvError::Fetch("response read failed or timed out".into()))?
            {
                if chunk.len() > BODY_LIMIT - bytes.len() {
                    return Err(EnvError::Fetch("response exceeds 8 MiB".into()));
                }
                bytes.extend_from_slice(&chunk);
            }
            serde_json::from_slice(&bytes)
                .map_err(|_| EnvError::Serde("response deserialization failed".into()))
        }
        .boxed_env()
    }

    fn get_storage<T: for<'de> Deserialize<'de> + ConditionalSend + 'static>(
        key: &str,
    ) -> TryEnvFuture<Option<T>> {
        let bytes = Self::state()
            .storage
            .lock()
            .map(|storage| storage.get(key).cloned())
            .map_err(|_| EnvError::StorageUnavailable);
        async move {
            let bytes = bytes?;
            bytes
                .map(|bytes| serde_json::from_slice(&bytes))
                .transpose()
                .map_err(|_| EnvError::StorageReadError("invalid stored JSON".into()))
        }
        .boxed_env()
    }

    fn set_storage<T: Serialize>(key: &str, value: Option<&T>) -> TryEnvFuture<()> {
        let key = key.to_owned();
        let bytes = value.map(serde_json::to_vec).transpose();
        async move {
            let bytes = bytes
                .map_err(|_| EnvError::StorageWriteError("storage serialization failed".into()))?;
            let mut storage = Self::state()
                .storage
                .lock()
                .map_err(|_| EnvError::StorageUnavailable)?;
            if let Some(bytes) = bytes {
                storage.insert(key, bytes);
            } else {
                storage.remove(&key);
            }
            Ok(())
        }
        .boxed_env()
    }

    fn exec_concurrent<F: Future<Output = ()> + ConditionalSend + 'static>(future: F) {
        tokio::spawn(future);
    }

    fn exec_sequential<F: Future<Output = ()> + ConditionalSend + 'static>(future: F) {
        assert!(
            Self::state().sequential.send(future.boxed_env()).is_ok(),
            "sequential executor stopped"
        );
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

#[derive(Clone, stremio_core::Model)]
#[model(NativeEnv)]
pub struct CoreModel {
    pub ctx: Ctx,
}

pub struct CoreSession {
    runtime: Runtime<NativeEnv, CoreModel>,
    events: futures::channel::mpsc::Receiver<RuntimeEvent<NativeEnv, CoreModel>>,
}

impl CoreSession {
    pub async fn new() -> Result<Self, &'static str> {
        NativeEnv::migrate_storage_schema()
            .await
            .map_err(|_| "storage migration failed")?;
        let ctx = Ctx::new(
            Profile::default(),
            LibraryBucket::default(),
            StreamsBucket::default(),
            ServerUrlsBucket::new::<NativeEnv>(None),
            NotificationsBucket::new::<NativeEnv>(None, vec![]),
            SearchHistoryBucket::default(),
            DismissedEventsBucket::default(),
        );
        let (runtime, events) = Runtime::new(CoreModel { ctx }, vec![], 1024);
        Ok(Self { runtime, events })
    }

    pub fn profile(&self) -> Result<Profile, &'static str> {
        self.runtime
            .model()
            .map(|model| model.ctx.profile.clone())
            .map_err(|_| "model lock failed")
    }

    pub async fn sign_in(
        &mut self,
        email: String,
        password: String,
    ) -> Result<Profile, &'static str> {
        self.runtime.dispatch(RuntimeAction {
            field: None,
            action: Action::Ctx(ActionCtx::Authenticate(AuthRequest::Login {
                email,
                password,
                facebook: false,
            })),
        });
        tokio::time::timeout(TIMEOUT, async {
            let mut authenticated = false;
            let mut addons_ready = false;
            let mut library_ready = false;
            while let Some(event) = self.events.next().await {
                match event {
                    RuntimeEvent::CoreEvent(Event::UserAuthenticated { .. }) => {
                        authenticated = true
                    }
                    RuntimeEvent::CoreEvent(Event::UserAddonsLocked {
                        addons_locked: false,
                    }) => addons_ready = true,
                    RuntimeEvent::CoreEvent(Event::UserLibraryMissing {
                        library_missing: false,
                    }) => library_ready = true,
                    RuntimeEvent::CoreEvent(Event::Error { .. }) => {
                        return Err("core reported an error during sign-in");
                    }
                    _ => {}
                }
                if authenticated && addons_ready && library_ready {
                    return self.profile();
                }
            }
            Err("runtime event stream ended during sign-in")
        })
        .await
        .map_err(|_| "sign-in timed out after 30 seconds")?
    }
}

pub fn addon_lines(addons: &[Descriptor]) -> Vec<String> {
    addons
        .iter()
        .map(|addon| {
            let manifest = &addon.manifest;
            let resources = ["catalog", "meta", "stream", "subtitles"]
                .into_iter()
                .filter(|name| {
                    (*name == "catalog" && !manifest.catalogs.is_empty())
                        || manifest.resources.iter().any(|resource| match resource {
                            ManifestResource::Short(resource) => resource == name,
                            ManifestResource::Full { name: resource, .. } => resource == name,
                        })
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "addon name={:?} id={:?} version={} host={} resources=[{}]",
                manifest.name,
                manifest.id,
                manifest.version,
                addon.transport_url.host_str().unwrap_or("(none)"),
                resources
            )
        })
        .collect()
}

pub async fn first_movie_catalog(addons: &[Descriptor]) -> Result<Vec<String>, &'static str> {
    let (addon, catalog) = addons
        .iter()
        .find_map(|addon| {
            addon
                .manifest
                .catalogs
                .iter()
                .find(|catalog| catalog.r#type == "movie")
                .map(|catalog| (addon, catalog))
        })
        .ok_or("no movie catalog found")?;
    let request = ResourceRequest::new(
        addon.transport_url.clone(),
        ResourcePath::without_extra("catalog", "movie", &catalog.id),
    );
    let future = NativeEnv::addon_transport(&request.base).resource(&request.path);
    let response = tokio::time::timeout(TIMEOUT, future)
        .await
        .map_err(|_| "catalog timed out after 30 seconds")?
        .map_err(|_| "catalog request failed")?;
    match response {
        ResourceResponse::Metas { metas } => {
            Ok(metas.into_iter().take(5).map(|item| item.name).collect())
        }
        ResourceResponse::MetasDetailed { metas_detailed } => Ok(metas_detailed
            .into_iter()
            .take(5)
            .map(|item| item.preview.name)
            .collect()),
        _ => Err("catalog returned a different resource type"),
    }
}
