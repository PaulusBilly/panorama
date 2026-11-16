//! A loopback relay between libmpv and YouTube's video servers. Every segment libmpv opens
//! itself costs a fresh connection and a redirect, a second or more each, and its HLS reader
//! opens the first one twice; through the relay it reads them from memory. The relay fetches
//! over pooled connections, starts on a clip's first segment as soon as the clip is known, and
//! stays a segment ahead of playback. Clips sit under unguessable paths on 127.0.0.1 only.
use crate::media::{lock, token};
use bytes::Bytes;
use futures::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use http::{Method, Request, Response, StatusCode, header};
use http_body_util::Full;
use hyper::{body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::{
    collections::HashMap,
    convert::Infallible,
    net::Ipv4Addr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::net::TcpListener;
use url::Url;

/// Largest segment taken; a 1080p segment of a few seconds is a few megabytes.
const MAX_SEGMENT: usize = 16 * 1024 * 1024;
/// Clips kept; a hover and its banner use two, and older ones are long finished.
const KEEP_CLIPS: usize = 8;
const SEGMENT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_REDIRECTS: usize = 3;
/// Segments kept behind the one asked for.
const KEEP_BEHIND: usize = 3;

type Fetch = Shared<BoxFuture<'static, Option<Bytes>>>;

struct Clip {
    playlist: Bytes,
    /// A fragmented MP4 stream's header and its download, fetched with the first segment.
    init: Option<Fetch>,
    /// Segments are MPEG-TS, or fragmented MP4 after a header.
    kind: &'static str,
    sources: Vec<Url>,
    /// Each segment's download, started ahead of need and dropped once played past.
    fetches: Mutex<Vec<Option<Fetch>>>,
    made: Instant,
}

pub(super) struct Relay {
    port: u16,
    http: reqwest::Client,
    clips: Mutex<HashMap<String, Arc<Clip>>>,
}

impl Relay {
    pub(super) async fn start() -> Option<Arc<Self>> {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .referer(false)
            .timeout(SEGMENT_TIMEOUT)
            // Segment links redirect to the server holding the video; only ever to another of
            // YouTube's video servers.
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() < MAX_REDIRECTS && is_video_server(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .ok()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.ok()?;
        let relay = Arc::new(Self {
            port: listener.local_addr().ok()?.port(),
            http,
            clips: Mutex::new(HashMap::new()),
        });
        let serving = Arc::downgrade(&relay);
        tokio::spawn(async move {
            while let Ok((stream, peer)) = listener.accept().await {
                let Some(relay) = serving.upgrade() else {
                    break;
                };
                if !peer.ip().is_loopback() {
                    continue;
                }
                tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        let relay = relay.clone();
                        async move { Ok::<_, Infallible>(relay.answer(request).await) }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        Some(relay)
    }

    /// Serves `segments` (seconds, link), after the header segment `init` if the stream has
    /// one, as a playlist and returns its loopback link. Starts downloading at once.
    pub(super) fn add(
        self: &Arc<Self>,
        init: Option<Url>,
        segments: &[(f64, Url)],
    ) -> Option<String> {
        let longest = segments
            .iter()
            .map(|(seconds, _)| seconds.ceil() as u64)
            .max()?;
        let (version, kind, extension) = match init {
            Some(_) => (6, "video/mp4", "m4s"),
            None => (3, "video/mp2t", "ts"),
        };
        let mut playlist = format!(
            "#EXTM3U\n#EXT-X-VERSION:{version}\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-TARGETDURATION:{longest}\n"
        );
        if init.is_some() {
            playlist.push_str("#EXT-X-MAP:URI=\"init.mp4\"\n");
        }
        for (index, (seconds, _)) in segments.iter().enumerate() {
            playlist.push_str(&format!("#EXTINF:{seconds},\n{index}.{extension}\n"));
        }
        playlist.push_str("#EXT-X-ENDLIST\n");
        let token = token().ok()?;
        let clip = Arc::new(Clip {
            playlist: Bytes::from(playlist),
            init: init.map(|url| self.download(url)),
            kind,
            sources: segments.iter().map(|(_, url)| url.clone()).collect(),
            fetches: Mutex::new(vec![None; segments.len()]),
            made: Instant::now(),
        });
        {
            let mut clips = lock(&self.clips);
            while clips.len() >= KEEP_CLIPS {
                let Some(oldest) = clips
                    .iter()
                    .min_by_key(|(_, clip)| clip.made)
                    .map(|(token, _)| token.clone())
                else {
                    break;
                };
                clips.remove(&oldest);
            }
            clips.insert(token.clone(), clip.clone());
        }
        self.fetch(&clip, 0);
        Some(format!("http://127.0.0.1:{}/{token}/index.m3u8", self.port))
    }

    /// Segment `index`'s download, started if it is not under way. Segments well behind are
    /// let go: playback has moved on. A few stay, since libmpv's HLS reader goes back to the
    /// first segment after probing it to seek within it.
    fn fetch(&self, clip: &Clip, index: usize) -> Option<Fetch> {
        let url = clip.sources.get(index)?.clone();
        let mut fetches = lock(&clip.fetches);
        for done in fetches.iter_mut().take(index.saturating_sub(KEEP_BEHIND)) {
            *done = None;
        }
        if let Some(fetch) = &fetches[index] {
            return Some(fetch.clone());
        }
        let fetch = self.download(url);
        fetches[index] = Some(fetch.clone());
        Some(fetch)
    }

    /// Downloads `url`, starting now rather than when first awaited.
    fn download(&self, url: Url) -> Fetch {
        let http = self.http.clone();
        let fetch = async move { download(&http, url).await }.boxed().shared();
        tokio::spawn(fetch.clone());
        fetch
    }

    async fn answer(self: Arc<Self>, request: Request<Incoming>) -> Response<Full<Bytes>> {
        if request.method() != Method::GET {
            return respond(StatusCode::METHOD_NOT_ALLOWED, "", Bytes::new());
        }
        let Some((token, file)) = request
            .uri()
            .path()
            .strip_prefix('/')
            .and_then(|path| path.split_once('/'))
        else {
            return respond(StatusCode::NOT_FOUND, "", Bytes::new());
        };
        let Some(clip) = lock(&self.clips).get(token).cloned() else {
            return respond(StatusCode::NOT_FOUND, "", Bytes::new());
        };
        if file == "index.m3u8" {
            return respond(
                StatusCode::OK,
                "application/vnd.apple.mpegurl",
                clip.playlist.clone(),
            );
        }
        if file == "init.mp4"
            && let Some(init) = &clip.init
        {
            return match init.clone().await {
                Some(bytes) => respond(StatusCode::OK, "video/mp4", bytes),
                None => respond(StatusCode::BAD_GATEWAY, "", Bytes::new()),
            };
        }
        let Some(index) = file
            .strip_suffix(".ts")
            .or_else(|| file.strip_suffix(".m4s"))
            .and_then(|index| index.parse::<usize>().ok())
        else {
            return respond(StatusCode::NOT_FOUND, "", Bytes::new());
        };
        let Some(fetch) = self.fetch(&clip, index) else {
            return respond(StatusCode::NOT_FOUND, "", Bytes::new());
        };
        // Stay a segment ahead.
        self.fetch(&clip, index + 1);
        match fetch.await {
            Some(bytes) => respond(StatusCode::OK, clip.kind, bytes),
            None => {
                // Let a later request try again.
                if let Some(slot) = lock(&clip.fetches).get_mut(index) {
                    *slot = None;
                }
                respond(StatusCode::BAD_GATEWAY, "", Bytes::new())
            }
        }
    }
}

pub(super) fn is_video_server(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.ends_with(".googlevideo.com"))
}

async fn download(http: &reqwest::Client, url: Url) -> Option<Bytes> {
    let mut response = http.get(url).send().await.ok()?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_SEGMENT as u64)
    {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > MAX_SEGMENT {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(Bytes::from(body))
}

fn respond(status: StatusCode, kind: &str, body: Bytes) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(body));
    *response.status_mut() = status;
    if !kind.is_empty()
        && let Ok(kind) = header::HeaderValue::from_str(kind)
    {
        response.headers_mut().insert(header::CONTENT_TYPE, kind);
    }
    response
}
