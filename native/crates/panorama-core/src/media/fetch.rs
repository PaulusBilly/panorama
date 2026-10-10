//! Manual HTTP redirects ported from `desktop/main/media-fetch.ts`.

use super::MediaError;
use bytes::Bytes;
use futures::{Stream, StreamExt, future::BoxFuture};
use http::{HeaderMap, Method};
use std::{fmt, net::IpAddr, pin::Pin, sync::Arc};

mod destination;
use destination::{SafeDns, forbidden_address, transport_error};

/// Credential-free HTTP source with redacted diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub struct MediaSource(reqwest::Url);

impl MediaSource {
    /// Validates scheme and rejects URL credentials.
    pub fn new(value: &str) -> Result<Self, MediaError> {
        let url = reqwest::Url::parse(value).map_err(|_| MediaError::InvalidDestination)?;
        Self::validate(url)
    }
    fn validate(url: reqwest::Url) -> Result<Self, MediaError> {
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_none()
            || url
                .host_str()
                .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
                .is_some_and(forbidden_address)
        {
            return Err(MediaError::InvalidDestination);
        }
        Ok(Self(url))
    }
    /// Returns the secret transport URL for the HTTP client or resolver only.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
impl fmt::Debug for MediaSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MediaSource([REDACTED])")
    }
}

/// Streaming body; all errors must be sanitized MediaError values.
pub type MediaBody = Pin<Box<dyn Stream<Item = Result<Bytes, MediaError>> + Send>>;

/// Injected upstream request, independent of Electron and reqwest.
#[derive(Clone, Debug)]
pub struct FetchRequest {
    /// GET or HEAD.
    pub method: Method,
    /// Range and representation validators.
    pub headers: HeaderMap,
}

/// Upstream response with a validated final redirect URL.
pub struct FetchResponse {
    /// HTTP status code.
    pub status: u16,
    /// Upstream representation headers.
    pub headers: HeaderMap,
    /// Streaming response body.
    pub body: MediaBody,
    /// Final destination, redacted by MediaSource's Debug implementation.
    pub final_source: MediaSource,
}

/// Injectable upstream transport. Dropping its future/body cancels the request.
pub trait MediaFetch: Send + Sync {
    /// Issues one request with manual redirect validation.
    fn fetch(
        &self,
        source: MediaSource,
        request: FetchRequest,
    ) -> BoxFuture<'static, Result<FetchResponse, MediaError>>;
}

/// Reqwest transport using rustls, no cookie jar and no automatic redirects.
pub struct HttpMediaFetch {
    client: reqwest::Client,
}
impl HttpMediaFetch {
    /// Constructs a transport with no decompression or cookies.
    pub fn new() -> Result<Self, MediaError> {
        Self::build(Self::builder())
    }
    fn builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .use_rustls_tls()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .no_proxy()
            .dns_resolver(Arc::new(SafeDns))
            .redirect(reqwest::redirect::Policy::none())
    }
    fn build(builder: reqwest::ClientBuilder) -> Result<Self, MediaError> {
        let client = builder.build().map_err(|_| MediaError::Transport)?;
        Ok(Self { client })
    }
    #[cfg(test)]
    pub(crate) fn for_test(address: std::net::SocketAddr) -> Result<Self, MediaError> {
        Self::build(Self::builder().resolve("upstream.invalid", address))
    }
}
impl MediaFetch for HttpMediaFetch {
    fn fetch(
        &self,
        source: MediaSource,
        request: FetchRequest,
    ) -> BoxFuture<'static, Result<FetchResponse, MediaError>> {
        let client = self.client.clone();
        Box::pin(async move {
            let mut source = source;
            for hops in 0..=10 {
                let mut headers = request.headers.clone();
                headers.remove(http::header::COOKIE);
                headers.remove(http::header::AUTHORIZATION);
                headers.insert(
                    http::header::CACHE_CONTROL,
                    http::HeaderValue::from_static("no-store"),
                );
                let response = client
                    .request(request.method.clone(), source.0.clone())
                    .headers(headers)
                    .send()
                    .await
                    .map_err(transport_error)?;
                let status = response.status().as_u16();
                if matches!(status, 301 | 302 | 303 | 307 | 308)
                    && let Some(location) = super::range::header(response.headers(), "location")
                {
                    if hops == 10 {
                        return Err(MediaError::RedirectLimit);
                    }
                    source = MediaSource::validate(
                        source
                            .0
                            .join(location)
                            .map_err(|_| MediaError::InvalidDestination)?,
                    )?;
                    continue;
                }
                let headers = response.headers().clone();
                let body = Box::pin(
                    response
                        .bytes_stream()
                        .map(|part| part.map_err(|_| MediaError::Transport)),
                );
                return Ok(FetchResponse {
                    status,
                    headers,
                    body,
                    final_source: source,
                });
            }
            Err(MediaError::RedirectLimit)
        })
    }
}

#[cfg(test)]
mod tests;

/// Caller-supplied link refresh, shared by all failed ranges in a burst.
pub trait MediaResolver: Send + Sync {
    /// Resolves the original source to a fresh credential-free HTTP URL.
    fn resolve(&self, source: MediaSource) -> BoxFuture<'static, Result<MediaSource, MediaError>>;
}

impl<F> MediaResolver for F
where
    F: Fn(MediaSource) -> BoxFuture<'static, Result<MediaSource, MediaError>> + Send + Sync,
{
    fn resolve(&self, source: MediaSource) -> BoxFuture<'static, Result<MediaSource, MediaError>> {
        self(source)
    }
}
