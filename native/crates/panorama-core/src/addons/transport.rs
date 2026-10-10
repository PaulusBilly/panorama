use std::cell::RefCell;

use futures::{FutureExt, future::BoxFuture};
use stremio_core::{
    runtime::{Env, EnvError},
    types::addon::{ResourceRequest, ResourceResponse},
};
use tracing::{
    instrument::WithSubscriber,
    subscriber::{NoSubscriber, with_default},
};

use super::FailureKind;
use crate::stremio::env::PanoramaEnv;

/// Transport output; raw responses are bounded and parsed with the core types.
pub enum TransportResponse {
    /// Raw bytes for custom transports and deterministic fakes.
    Bytes(Vec<u8>),
    /// Already parsed core response; production Env enforces the byte limit.
    Core(ResourceResponse),
    /// Core response with credits preserved before the core discards legacy fields.
    WithCredits {
        /// Parsed core resource.
        response: ResourceResponse,
        /// Bounded director names.
        director: Vec<String>,
        /// Bounded cast names.
        cast: Vec<String>,
    },
}

tokio::task_local! {
    static CREDITS: RefCell<super::sanitize::Credits>;
}

pub(crate) fn capture_metadata(bytes: &[u8]) {
    let _ = CREDITS.try_with(|credits| {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
            *credits.borrow_mut() = super::sanitize::credits(&value);
        }
    });
}

/// Injectable, secret-safe addon transport. Dropping its future cancels a request.
pub trait AddonTransport: Send + Sync + 'static {
    /// Fetches an unencoded resource request; use core path encoding in adapters.
    /// Return only sanitized failure categories, never raw transport errors.
    fn resource(
        &self,
        request: ResourceRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, FailureKind>>;
}

/// Production adapter using the core's transport and Panorama's bounded HTTPS Env.
#[derive(Clone, Copy, Default)]
pub struct CoreAddonTransport;

impl AddonTransport for CoreAddonTransport {
    fn resource(
        &self,
        request: ResourceRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, FailureKind>> {
        let path_bytes = request
            .path
            .resource
            .len()
            .saturating_add(request.path.r#type.len())
            .saturating_add(request.path.id.len())
            .saturating_add(request.path.extra.iter().fold(0usize, |length, extra| {
                length
                    .saturating_add(extra.name.len())
                    .saturating_add(extra.value.len())
                    .saturating_add(2)
            }))
            .saturating_add(32);
        let encoded_bound = request.base.as_str().len().saturating_add(
            path_bytes.saturating_mul(3).saturating_mul(
                request
                    .base
                    .as_str()
                    .matches("manifest.json")
                    .count()
                    .max(1),
            ),
        );
        if encoded_bound > 8192 {
            return futures::future::ready(Err(FailureKind::InvalidInput)).boxed();
        }
        let future = with_default(NoSubscriber::default(), || {
            PanoramaEnv::addon_transport(&request.base).resource(&request.path)
        });
        async move {
            CREDITS
                .scope(
                    RefCell::new(super::sanitize::Credits::default()),
                    async move {
                        future
                            .await
                            .map(|response| {
                                CREDITS.with(|credits| {
                                    let credits = credits.borrow();
                                    TransportResponse::WithCredits {
                                        response,
                                        director: credits.director.clone(),
                                        cast: credits.cast.clone(),
                                    }
                                })
                            })
                            .map_err(|error| match error {
                                EnvError::Serde(_) => FailureKind::InvalidResponse,
                                EnvError::Fetch(message) if message == "response exceeds 8 MiB" => {
                                    FailureKind::TooLarge
                                }
                                EnvError::Fetch(message) if message == "request timed out" => {
                                    FailureKind::Timeout
                                }
                                EnvError::Fetch(message) if message == "request failed" => {
                                    FailureKind::Offline
                                }
                                _ => FailureKind::Network,
                            })
                    },
                )
                .await
        }
        .with_subscriber(NoSubscriber::default())
        .boxed()
    }
}
