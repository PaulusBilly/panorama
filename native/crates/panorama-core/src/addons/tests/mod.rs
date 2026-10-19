use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures::{FutureExt, StreamExt, future::BoxFuture};
use serde_json::{Value, json};
use stremio_core::types::addon::{ResourcePath, ResourceRequest};

use super::*;

mod cache;
mod catalog;
mod details;
mod manifests;
mod search;
mod security;

#[derive(Clone)]
struct Reply {
    delay: Duration,
    value: Result<Vec<u8>, FailureKind>,
}

impl Reply {
    fn json(value: Value) -> Self {
        Self {
            delay: Duration::ZERO,
            value: Ok(serde_json::to_vec(&value).unwrap()),
        }
    }

    fn error(kind: FailureKind) -> Self {
        Self {
            delay: Duration::ZERO,
            value: Err(kind),
        }
    }

    fn delayed(mut self, seconds: u64) -> Self {
        self.delay = Duration::from_secs(seconds);
        self
    }
}

#[derive(Clone, Default)]
struct Fake {
    replies: Arc<Mutex<Replies>>,
    calls: Arc<Mutex<Vec<(String, ResourcePath)>>>,
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
}

type Replies = HashMap<(String, String, String), Reply>;

struct Active(Arc<AtomicUsize>);

impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Fake {
    fn set(&self, host: &str, resource: &str, id: &str, reply: Reply) {
        self.replies
            .lock()
            .unwrap()
            .insert((host.into(), resource.into(), id.into()), reply);
    }

    fn hosts(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(host, _)| host.clone())
            .collect()
    }
}

impl AddonTransport for Fake {
    fn resource(
        &self,
        request: ResourceRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, FailureKind>> {
        let fake = self.clone();
        async move {
            let host = request.base.host_str().unwrap().to_owned();
            fake.calls
                .lock()
                .unwrap()
                .push((host.clone(), request.path.clone()));
            let reply = fake
                .replies
                .lock()
                .unwrap()
                .get(&(host, request.path.resource, request.path.id))
                .cloned()
                .unwrap_or_else(|| Reply::error(FailureKind::Network));
            let active = fake.active.fetch_add(1, Ordering::SeqCst) + 1;
            let _guard = Active(fake.active.clone());
            fake.maximum.fetch_max(active, Ordering::SeqCst);
            if !reply.delay.is_zero() {
                tokio::time::sleep(reply.delay).await;
            }
            reply.value.map(TransportResponse::Bytes)
        }
        .boxed()
    }
}

fn addon(host: &str, id: &str, catalogs: Value, resources: Value, prefixes: Value) -> Descriptor {
    Descriptor::from_core(
        serde_json::from_value(json!({
            "transportUrl": format!("https://{host}/secret-debrid-key/manifest.json?token=secret"),
            "manifest": {"id": id, "version": "1.0.0", "name": host, "types": ["movie"],
                "resources": resources, "idPrefixes": prefixes, "catalogs": catalogs}
        }))
        .unwrap(),
    )
}

fn home_addon(host: &str) -> Descriptor {
    addon(
        host,
        host,
        json!([{"id":"top","type":"movie","name":"Movies"}]),
        json!(["meta", "stream"]),
        Value::Null,
    )
}

fn movie(id: &str, name: &str) -> Value {
    json!({"id":id,"type":"movie","name":name,"poster":"https://images.test/poster.jpg","releaseInfo":"2024"})
}

fn page(id: &str, name: &str) -> Reply {
    Reply::json(json!({"metas":[movie(id, name)]}))
}

async fn client(fake: &Fake) -> AddonClient {
    AddonClient::new(Arc::new(Store::open_in_memory().unwrap()), fake.clone())
        .await
        .unwrap()
}

fn fresh_page(events: &[ResourceEvent<Page>]) -> &Page {
    events
        .iter()
        .find_map(|event| match event {
            ResourceEvent::Fresh(page) => Some(page),
            _ => None,
        })
        .unwrap()
}
