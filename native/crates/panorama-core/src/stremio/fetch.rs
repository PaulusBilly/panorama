use std::{sync::Arc, time::Duration};

use http::{Method, Request};
use serde::{Deserialize, Serialize};
use stremio_core::{
    constants::API_URL,
    runtime::{ConditionalSend, EnvError},
};
use url::{Host, Origin, Url};

use super::{CoreError, CoreErrorKind, env::State};

pub(super) const BODY_LIMIT: usize = 8 * 1024 * 1024;
pub(super) const TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn redirect_error(
    url: &Url,
    previous: &[Url],
    override_origin: Option<&Origin>,
) -> Option<&'static str> {
    if previous.len() >= 10 {
        return Some("redirect limit reached");
    }
    if url.scheme() != "https" {
        return Some("HTTPS is required for redirects");
    }
    if let Some(first) = previous.first()
        && (first.origin() == API_URL.origin() || override_origin == Some(&first.origin()))
        && url.origin() != first.origin()
    {
        return Some("API redirects must keep the same origin");
    }
    None
}

pub(super) fn client(api_base: Option<&Url>) -> Result<reqwest::Client, CoreError> {
    client_builder(api_base)?
        .build()
        .map_err(|_| CoreErrorKind::Environment.into())
}

pub(super) fn client_builder(api_base: Option<&Url>) -> Result<reqwest::ClientBuilder, CoreError> {
    if let Some(base) = api_base {
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
            return Err(CoreErrorKind::Environment.into());
        }
    }
    let override_origin = api_base.map(Url::origin);
    Ok(reqwest::Client::builder()
        .use_rustls_tls()
        .referer(false)
        .https_only(api_base.is_none())
        .redirect(reqwest::redirect::Policy::custom(
            move |attempt| match redirect_error(
                attempt.url(),
                attempt.previous(),
                override_origin.as_ref(),
            ) {
                Some(reason) => attempt.error(reason),
                None => attempt.follow(),
            },
        ))
        .timeout(TIMEOUT))
}

pub(super) async fn fetch<
    IN: Serialize + ConditionalSend + 'static,
    OUT: for<'de> Deserialize<'de> + ConditionalSend + 'static,
>(
    state: Arc<State>,
    request: Request<IN>,
) -> Result<OUT, EnvError> {
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
    let mut response = builder.send().await.map_err(network_error)?;
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
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if chunk.len() > BODY_LIMIT - bytes.len() {
            return Err(EnvError::Fetch("response exceeds 8 MiB".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| EnvError::Serde("response deserialization failed".into()))
}

fn network_error(error: reqwest::Error) -> EnvError {
    EnvError::Fetch(
        if error.is_timeout() {
            "request timed out"
        } else {
            "request failed"
        }
        .into(),
    )
}
