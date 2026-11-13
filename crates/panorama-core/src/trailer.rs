//! Trailer clips for the hover preview: a film's YouTube trailer from Cinemeta, turned into a
//! video-only HLS playlist that libmpv can open directly. Asks YouTube the way its visionOS app
//! does, whose answer carries plain playlist links (no signature puzzles to solve), so a lookup
//! is two small requests instead of a bundled downloader.
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use url::Url;

const TIMEOUT: Duration = Duration::from_secs(8);
const CINEMETA: &str = "https://v3-cinemeta.strem.io/meta";
const YOUTUBE: &str = "https://www.youtube.com";
/// The client YouTube's own visionOS app identifies as. YouTube retires old clients now and then;
/// when lookups start failing with `Refused`, these are what to update.
const CLIENT_NAME: &str = "VISIONOS";
const CLIENT_ID: &str = "101";
const CLIENT_VERSION: &str = "1.02";
const CLIENT_OS: &str = "26.5.23O471";
const CLIENT_DEVICE: &str = "RealityDevice17,1";
const CLIENT_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 \
     (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
/// A visitor ID is good for hours; refresh well before.
const VISITOR_FOR: Duration = Duration::from_secs(60 * 60);
/// Playlist links expire after about six hours.
const PLAYLIST_FOR: Duration = Duration::from_secs(60 * 60);
/// Largest response read from any of these services.
const MAX_BODY: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrailerError {
    /// The film has no trailer, or YouTube will not play it here.
    Unavailable,
    /// YouTube refused this client.
    Refused,
    Network,
}

/// Looks up and caches trailers. Clone-free: share it behind an `Arc`.
pub struct TrailerClient {
    http: reqwest::Client,
    visitor: Mutex<Option<(String, Instant)>>,
    /// Film ID to YouTube video ID, `None` for a film without one.
    videos: Mutex<HashMap<String, Option<String>>>,
    playlists: Mutex<HashMap<(String, u32), (Url, Instant)>>,
}

impl TrailerClient {
    pub fn new() -> Result<Self, TrailerError> {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .referer(false)
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| TrailerError::Network)?;
        Ok(Self {
            http,
            visitor: Mutex::new(None),
            videos: Mutex::new(HashMap::new()),
            playlists: Mutex::new(HashMap::new()),
        })
    }

    /// A video-only playlist of the film's trailer, about `height` pixels tall. `kind` is the
    /// addon type (`movie` or `series`); only IMDb IDs have Cinemeta trailers.
    pub async fn playlist(
        &self,
        kind: &str,
        film_id: &str,
        height: u32,
    ) -> Result<Url, TrailerError> {
        let video = self.video(kind, film_id).await?;
        let key = (video.clone(), height);
        if let Some((url, at)) = self
            .playlists
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned())
            && at.elapsed() < PLAYLIST_FOR
        {
            return Ok(url);
        }
        let master = self.master(&video).await?;
        let text = self.text(self.http.get(master.as_str())).await?;
        let url = best_variant(&text, height).ok_or(TrailerError::Unavailable)?;
        if let Ok(mut cache) = self.playlists.lock() {
            cache.insert(key, (url.clone(), Instant::now()));
        }
        Ok(url)
    }

    /// The film's YouTube trailer ID from Cinemeta, remembered for the session.
    async fn video(&self, kind: &str, film_id: &str) -> Result<String, TrailerError> {
        if let Some(known) = self
            .videos
            .lock()
            .ok()
            .and_then(|cache| cache.get(film_id).cloned())
        {
            return known.ok_or(TrailerError::Unavailable);
        }
        if !is_imdb_id(film_id) || !matches!(kind, "movie" | "series") {
            return Err(TrailerError::Unavailable);
        }
        let url = format!("{CINEMETA}/{kind}/{film_id}.json");
        let found = match self.json(self.http.get(url)).await {
            Ok(meta) => trailer_id(&meta),
            Err(TrailerError::Unavailable) => None,
            Err(error) => return Err(error),
        };
        if let Ok(mut cache) = self.videos.lock() {
            cache.insert(film_id.to_owned(), found.clone());
        }
        found.ok_or(TrailerError::Unavailable)
    }

    /// The trailer's master HLS playlist.
    async fn master(&self, video: &str) -> Result<Url, TrailerError> {
        let visitor = self.visitor().await?;
        let body = serde_json::json!({
            "context": { "client": {
                "clientName": CLIENT_NAME,
                "clientVersion": CLIENT_VERSION,
                "deviceMake": "Apple",
                "deviceModel": CLIENT_DEVICE,
                "userAgent": CLIENT_AGENT,
                "osName": "visionOS",
                "osVersion": CLIENT_OS,
                "hl": "en",
                "visitorData": visitor,
            }},
            "videoId": video,
            "contentCheckOk": true,
            "racyCheckOk": true,
        });
        let request = self
            .http
            .post(format!("{YOUTUBE}/youtubei/v1/player?prettyPrint=false"))
            .header("User-Agent", CLIENT_AGENT)
            .header("X-Youtube-Client-Name", CLIENT_ID)
            .header("X-Youtube-Client-Version", CLIENT_VERSION)
            .header("X-Goog-Visitor-Id", &visitor)
            .header("Origin", YOUTUBE)
            .json(&body);
        let player = self.json(request).await?;
        if player
            .pointer("/playabilityStatus/status")
            .and_then(Value::as_str)
            == Some("LOGIN_REQUIRED")
        {
            // A stale visitor ID reads as a sign-in wall; ask for a new one next time.
            if let Ok(mut visitor) = self.visitor.lock() {
                *visitor = None;
            }
            return Err(TrailerError::Refused);
        }
        master_playlist(&player).ok_or(TrailerError::Unavailable)
    }

    async fn visitor(&self) -> Result<String, TrailerError> {
        if let Some((id, at)) = self.visitor.lock().ok().and_then(|cached| cached.clone())
            && at.elapsed() < VISITOR_FOR
        {
            return Ok(id);
        }
        let text = self
            .text(
                self.http
                    .get(format!("{YOUTUBE}/sw.js_data"))
                    .header("User-Agent", CLIENT_AGENT),
            )
            .await?;
        let id = visitor_id(&text).ok_or(TrailerError::Refused)?;
        if let Ok(mut cached) = self.visitor.lock() {
            *cached = Some((id.clone(), Instant::now()));
        }
        Ok(id)
    }

    async fn json(&self, request: reqwest::RequestBuilder) -> Result<Value, TrailerError> {
        let text = self.text(request).await?;
        serde_json::from_str(&text).map_err(|_| TrailerError::Unavailable)
    }

    async fn text(&self, request: reqwest::RequestBuilder) -> Result<String, TrailerError> {
        let mut response = request.send().await.map_err(|_| TrailerError::Network)?;
        match response.status().as_u16() {
            200 => {}
            404 => return Err(TrailerError::Unavailable),
            403 | 429 => return Err(TrailerError::Refused),
            _ => return Err(TrailerError::Network),
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| TrailerError::Network)? {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(TrailerError::Network);
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).map_err(|_| TrailerError::Network)
    }
}

fn is_imdb_id(id: &str) -> bool {
    id.strip_prefix("tt").is_some_and(|digits| {
        (5..=10).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

/// YouTube video IDs are eleven URL-safe base64 characters.
fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A Cinemeta meta's first trailer: `trailerStreams[].ytId`, or the older `trailers[].source`.
fn trailer_id(meta: &Value) -> Option<String> {
    let meta = meta.get("meta").unwrap_or(meta);
    let streams = meta
        .get("trailerStreams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|stream| stream.get("ytId").and_then(Value::as_str));
    let legacy = meta
        .get("trailers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|trailer| trailer.get("source").and_then(Value::as_str));
    streams
        .chain(legacy)
        .find(|id| is_video_id(id))
        .map(str::to_owned)
}

/// The visitor ID in `sw.js_data`: a JSON array after a `)]}'` guard, holding a string that
/// starts `Cg` (a base64 protobuf).
fn visitor_id(text: &str) -> Option<String> {
    let start = text.find('[')?;
    let value: Value = serde_json::from_str(&text[start..]).ok()?;
    fn find(value: &Value) -> Option<&str> {
        match value {
            Value::String(text)
                if text.starts_with("Cg") && text.len() > 20 && text.len() < 1024 =>
            {
                Some(text)
            }
            Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    find(&value)
        .filter(|id| {
            id.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"%-_=".contains(&b))
        })
        .map(str::to_owned)
}

/// The player response's master playlist, only from YouTube's manifest host.
fn master_playlist(player: &Value) -> Option<Url> {
    if player
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        != Some("OK")
    {
        return None;
    }
    let url = Url::parse(player.pointer("/streamingData/hlsManifestUrl")?.as_str()?).ok()?;
    is_manifest(&url).then_some(url)
}

fn is_manifest(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some("manifest.googlevideo.com")
}

/// Picks a video variant from a master playlist: H.264 (cheap to decode) at the smallest height
/// covering `height`, else the tallest below it. YouTube's variants are video only, with audio
/// in separate renditions, so the clip downloads no sound.
fn best_variant(master: &str, height: u32) -> Option<Url> {
    let mut variants = Vec::new();
    let mut lines = master.lines();
    while let Some(line) = lines.next() {
        let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") else {
            continue;
        };
        let Some(uri) = lines.next().map(str::trim) else {
            break;
        };
        let Some(tall) = attribute(attributes, "RESOLUTION")
            .and_then(|size| size.split_once('x'))
            .and_then(|(_, tall)| tall.parse::<u32>().ok())
        else {
            continue;
        };
        let h264 = attribute(attributes, "CODECS").is_some_and(|codecs| codecs.contains("avc1"));
        let Ok(url) = Url::parse(uri) else {
            continue;
        };
        if h264 && is_manifest(&url) {
            variants.push((tall, url));
        }
    }
    let covering = variants
        .iter()
        .filter(|(tall, _)| *tall >= height)
        .min_by_key(|(tall, _)| *tall);
    covering
        .or_else(|| variants.iter().max_by_key(|(tall, _)| *tall))
        .map(|(_, url)| url.clone())
}

/// An attribute of an HLS tag, unquoted. Quoted values may contain commas.
fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attributes;
    while !rest.is_empty() {
        let (key, after) = rest.split_once('=')?;
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"')?;
            (&quoted[..end], quoted[end + 1..].trim_start_matches(','))
        } else {
            match after.split_once(',') {
                Some((value, next)) => (value, next),
                None => (after, ""),
            }
        };
        if key.trim() == name {
            return Some(value);
        }
        rest = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MASTER: &str = r#"#EXTM3U
#EXT-X-INDEPENDENT-SEGMENTS
#EXT-X-MEDIA:URI="https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/234/a.m3u8",TYPE=AUDIO,GROUP-ID="234"
#EXT-X-STREAM-INF:BANDWIDTH=224299,CODECS="avc1.4D4015,mp4a.40.5",RESOLUTION=426x240,FRAME-RATE=24,AUDIO="233"
https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/229/v240.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=752962,CODECS="avc1.4D401E,mp4a.40.2",RESOLUTION=854x480,FRAME-RATE=24,AUDIO="234"
https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/231/v480.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=1043403,CODECS="avc1.4D401F,mp4a.40.2",RESOLUTION=1280x720,FRAME-RATE=24,AUDIO="234"
https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/232/v720.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=338123,CODECS="vp09.00.20.08,mp4a.40.2",RESOLUTION=854x480,FRAME-RATE=24,AUDIO="234"
https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/244/vp9.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=9,CODECS="avc1.4D401F",RESOLUTION=640x360
https://evil.example/v360.m3u8
"#;

    fn tail(url: Option<Url>) -> Option<String> {
        url.and_then(|url| url.path_segments()?.next_back().map(str::to_owned))
    }

    #[test]
    fn variants_cover_the_height_with_h264_from_youtube() {
        assert_eq!(
            tail(best_variant(MASTER, 400)).as_deref(),
            Some("v480.m3u8")
        );
        assert_eq!(
            tail(best_variant(MASTER, 480)).as_deref(),
            Some("v480.m3u8")
        );
        assert_eq!(
            tail(best_variant(MASTER, 200)).as_deref(),
            Some("v240.m3u8")
        );
        assert_eq!(
            tail(best_variant(MASTER, 2000)).as_deref(),
            Some("v720.m3u8")
        );
        assert_eq!(best_variant("#EXTM3U\n", 480), None);
    }

    #[test]
    fn attributes_read_quoted_commas() {
        let line = r#"BANDWIDTH=1,CODECS="avc1.4D401E,mp4a.40.2",RESOLUTION=854x480"#;
        assert_eq!(attribute(line, "CODECS"), Some("avc1.4D401E,mp4a.40.2"));
        assert_eq!(attribute(line, "RESOLUTION"), Some("854x480"));
        assert_eq!(attribute(line, "AUDIO"), None);
    }

    #[test]
    fn trailers_come_from_streams_then_the_legacy_list() {
        let meta = json!({"meta": {"trailerStreams": [{"ytId": "cdx31ak4KbQ"}], "trailers": [{"source": "YoHD9XEInc0"}]}});
        assert_eq!(trailer_id(&meta).as_deref(), Some("cdx31ak4KbQ"));
        let legacy = json!({"meta": {"trailers": [{"source": "YoHD9XEInc0", "type": "Trailer"}]}});
        assert_eq!(trailer_id(&legacy).as_deref(), Some("YoHD9XEInc0"));
        let bad = json!({"meta": {"trailerStreams": [{"ytId": "../../etc"}]}});
        assert_eq!(trailer_id(&bad), None);
        assert_eq!(trailer_id(&json!({"meta": {}})), None);
    }

    #[test]
    fn visitor_ids_come_from_the_guarded_array() {
        let text = r#")]}'
[["yt.sw.adr",null,[[["id","ID",null,"2001:db8::1",null,"","","CgtON20zTzlvVXR1SSic4NzXBjIKCgJJRBIEGgAgYQ%3D%3D"]]]]]"#;
        assert_eq!(
            visitor_id(text).as_deref(),
            Some("CgtON20zTzlvVXR1SSic4NzXBjIKCgJJRBIEGgAgYQ%3D%3D")
        );
        assert_eq!(visitor_id(")]}'\n[[\"nothing\"]]"), None);
        assert_eq!(visitor_id("not json"), None);
    }

    #[test]
    fn master_playlists_need_an_ok_answer_from_youtube() {
        let ok = json!({"playabilityStatus": {"status": "OK"}, "streamingData": {"hlsManifestUrl": "https://manifest.googlevideo.com/api/manifest/hls_variant/x"}});
        assert!(master_playlist(&ok).is_some());
        let elsewhere = json!({"playabilityStatus": {"status": "OK"}, "streamingData": {"hlsManifestUrl": "https://evil.example/x"}});
        assert_eq!(master_playlist(&elsewhere), None);
        let walled = json!({"playabilityStatus": {"status": "LOGIN_REQUIRED"}});
        assert_eq!(master_playlist(&walled), None);
    }

    #[test]
    fn only_imdb_ids_are_looked_up() {
        assert!(is_imdb_id("tt1375666"));
        assert!(!is_imdb_id("kitsu:1"));
        assert!(!is_imdb_id("tt12/../x"));
    }

    /// The whole lookup against the live services.
    #[tokio::test]
    #[ignore = "needs the network"]
    async fn a_live_trailer_resolves() {
        let client = TrailerClient::new().unwrap();
        let started = Instant::now();
        let url = client.playlist("movie", "tt1375666", 480).await.unwrap();
        eprintln!("{:?} {}", started.elapsed(), url.host_str().unwrap_or(""));
        assert!(is_manifest(&url));
    }
}
