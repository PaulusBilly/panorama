//! Trailer clips for the hover preview: a film's YouTube trailer from Cinemeta, turned into a
//! video-only HLS playlist that libmpv can open directly. Asks YouTube the way its visionOS app
//! does, whose answer carries plain playlist links (no signature puzzles to solve), so a lookup
//! is a few small requests instead of a bundled downloader. The stream's own playlist is read
//! here, cut to start where the clip does, and served to libmpv through a loopback relay
//! ([`relay`]) that has the segments ready before libmpv asks.
mod relay;

use relay::Relay;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
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
/// Where a clip starts, as a share of the trailer: past the rating card and studio logos that
/// trailers open on.
const START_SHARE: f64 = 0.08;
/// Widest stream taken. Clips are drawn at no more than 1080p, so wider streams add only
/// download and decoding; a 1440p stream drawn down is still the sharpest picture, since
/// YouTube gives it several times the bitrate of its 1080p ones.
const MAX_STREAM_WIDTH: u32 = 2560;

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
    playlists: Mutex<HashMap<String, (Variant, Instant)>>,
    /// A stream's own playlist, by its link.
    segments: Mutex<HashMap<Url, (String, Instant)>>,
    relay: tokio::sync::OnceCell<Option<Arc<Relay>>>,
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
            segments: Mutex::new(HashMap::new()),
            relay: tokio::sync::OnceCell::new(),
        })
    }

    /// The film's trailer at its best quality, ready for libmpv. `kind` is the addon type
    /// (`movie` or `series`); only IMDb IDs have Cinemeta trailers.
    pub async fn clip(&self, kind: &str, film_id: &str) -> Result<Trailer, TrailerError> {
        let variant = self.playlist(kind, film_id).await?;
        let cached = self
            .segments
            .lock()
            .ok()
            .and_then(|cache| cache.get(&variant.url).cloned())
            .filter(|(_, at)| at.elapsed() < PLAYLIST_FOR);
        let media = match cached {
            Some((media, _)) => media,
            None => {
                let media = self.text(self.http.get(variant.url.as_str())).await?;
                if let Ok(mut cache) = self.segments.lock() {
                    cache.insert(variant.url.clone(), (media.clone(), Instant::now()));
                }
                media
            }
        };
        let cut = trim(&media).ok_or(TrailerError::Unavailable)?;
        let relay = self
            .relay
            .get_or_init(Relay::start)
            .await
            .as_ref()
            .ok_or(TrailerError::Network)?;
        Ok(Trailer {
            playlist: relay
                .add(cut.init, &cut.segments)
                .ok_or(TrailerError::Network)?,
            width: variant.width,
        })
    }

    /// The video-only playlist of the film's trailer at its best quality.
    async fn playlist(&self, kind: &str, film_id: &str) -> Result<Variant, TrailerError> {
        let video = self.video(kind, film_id).await?;
        if let Some((variant, at)) = self
            .playlists
            .lock()
            .ok()
            .and_then(|cache| cache.get(&video).cloned())
            && at.elapsed() < PLAYLIST_FOR
        {
            return Ok(variant);
        }
        let master = self.master(&video).await?;
        let text = self.text(self.http.get(master.as_str())).await?;
        let variant = best_variant(&text).ok_or(TrailerError::Unavailable)?;
        if let Ok(mut cache) = self.playlists.lock() {
            cache.insert(video, (variant.clone(), Instant::now()));
        }
        Ok(variant)
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

/// One stream of a trailer: its video-only playlist and how wide its picture is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Variant {
    pub url: Url,
    pub width: u32,
}

/// Picks the best video variant from a master playlist: the highest bitrate, H.264 or VP9, no
/// wider than `MAX_STREAM_WIDTH`. YouTube's variants are video only, with audio in separate
/// renditions, so the clip downloads no sound.
fn best_variant(master: &str) -> Option<Variant> {
    let mut best: Option<(u64, Variant)> = None;
    let mut lines = master.lines();
    while let Some(line) = lines.next() {
        let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") else {
            continue;
        };
        let Some(uri) = lines.next().map(str::trim) else {
            break;
        };
        let Some(wide) = attribute(attributes, "RESOLUTION")
            .and_then(|size| size.split_once('x'))
            .and_then(|(wide, _)| wide.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(bandwidth) =
            attribute(attributes, "BANDWIDTH").and_then(|bits| bits.parse::<u64>().ok())
        else {
            continue;
        };
        let codec = attribute(attributes, "CODECS").and_then(|codecs| codecs.split(',').next());
        let decodable =
            codec.is_some_and(|codec| codec.starts_with("avc1") || codec.starts_with("vp09"));
        let Ok(url) = Url::parse(uri) else {
            continue;
        };
        if decodable
            && wide <= MAX_STREAM_WIDTH
            && is_manifest(&url)
            && best.as_ref().is_none_or(|(most, _)| bandwidth > *most)
        {
            best = Some((bandwidth, Variant { url, width: wide }));
        }
    }
    best.map(|(_, variant)| variant)
}

/// A trailer ready for libmpv.
#[derive(Clone, Debug, PartialEq)]
pub struct Trailer {
    /// The stream's playlist from the segment the clip starts in, as a loopback link.
    pub playlist: String,
    pub width: u32,
}

/// A stream's segments from the one the clip starts in, and the header segment fragmented MP4
/// streams (VP9) start with.
#[derive(Debug, PartialEq)]
struct Cut {
    init: Option<Url>,
    segments: Vec<(f64, Url)>,
}

/// The stream's playlist cut to begin with the segment `START_SHARE` of the way in, so libmpv's
/// first download is the picture it shows. Only a plain playlist of whole segments from
/// YouTube's video servers passes: anything else, such as encryption or byte ranges, is refused
/// rather than handed on.
fn trim(media: &str) -> Option<Cut> {
    let mut init = None;
    let mut segments = Vec::new();
    let mut duration = None;
    let mut ended = false;
    for line in media.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(seconds) = line.strip_prefix("#EXTINF:") {
            let seconds = seconds.split(',').next()?.parse::<f64>().ok()?;
            (seconds.is_finite() && seconds > 0.0).then_some(())?;
            duration = Some(seconds);
        } else if let Some(map) = line.strip_prefix("#EXT-X-MAP:") {
            // Only a whole header file, and only one.
            (init.is_none() && attribute(map, "BYTERANGE").is_none()).then_some(())?;
            let url = Url::parse(attribute(map, "URI")?).ok()?;
            relay::is_video_server(&url).then_some(())?;
            init = Some(url);
        } else if line == "#EXT-X-ENDLIST" {
            ended = true;
        } else if line.starts_with('#') {
            const HEADER: [&str; 6] = [
                "#EXTM3U",
                "#EXT-X-VERSION:",
                "#EXT-X-PLAYLIST-TYPE:VOD",
                "#EXT-X-TARGETDURATION:",
                "#EXT-X-MEDIA-SEQUENCE:",
                "#EXT-X-INDEPENDENT-SEGMENTS",
            ];
            HEADER
                .iter()
                .any(|tag| line.starts_with(tag))
                .then_some(())?;
        } else {
            let url = Url::parse(line).ok()?;
            relay::is_video_server(&url).then_some(())?;
            segments.push((duration.take()?, url));
        }
    }
    (ended && !segments.is_empty()).then_some(())?;
    let total: f64 = segments.iter().map(|(seconds, _)| seconds).sum();
    let target = total * START_SHARE;
    let mut elapsed = 0.0;
    let first = segments
        .iter()
        .position(|(seconds, _)| {
            elapsed += seconds;
            elapsed > target
        })
        .unwrap_or(0);
    Some(Cut {
        init,
        segments: segments.split_off(first),
    })
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
#EXT-X-STREAM-INF:BANDWIDTH=1765000,CODECS="vp09.00.40.08,mp4a.40.2",RESOLUTION=1920x1080,FRAME-RATE=24,AUDIO="234"
https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/248/vp9-1080.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=3350000,CODECS="avc1.640028,mp4a.40.2",RESOLUTION=1920x1080,FRAME-RATE=24,AUDIO="234"
https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/137/v1080.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=6008533,CODECS="vp09.00.50.08,mp4a.40.2",RESOLUTION=2560x1440,FRAME-RATE=24,AUDIO="234"
https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/271/vp9-1440.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=16460000,CODECS="vp09.00.50.08,mp4a.40.2",RESOLUTION=3840x2160,FRAME-RATE=24,AUDIO="234"
https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/313/vp9-2160.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=9000000,CODECS="av01.0.08M.08",RESOLUTION=1920x1080
https://manifest.googlevideo.com/api/manifest/hls_playlist/itag/399/av1.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=99000000,CODECS="avc1.4D401F",RESOLUTION=640x360
https://evil.example/v360.m3u8
"#;

    fn tail(variant: Option<Variant>) -> Option<String> {
        variant.and_then(|variant| {
            variant
                .url
                .path_segments()?
                .next_back()
                .map(|name| format!("{name}@{}", variant.width))
        })
    }

    #[test]
    fn the_best_stream_is_the_richest_up_to_1440p() {
        // Not 4K (too wide), AV1 (not decoded) or another host, however rich.
        assert_eq!(
            tail(best_variant(MASTER)).as_deref(),
            Some("vp9-1440.m3u8@2560")
        );
        // Without VP9, H.264 1080p.
        let mut h264 = String::new();
        let mut lines = MASTER.lines();
        while let Some(line) = lines.next() {
            if line.contains("vp09") {
                lines.next();
            } else {
                h264.push_str(&format!("{line}\n"));
            }
        }
        assert_eq!(
            tail(best_variant(&h264)).as_deref(),
            Some("v1080.m3u8@1920")
        );
        assert_eq!(best_variant("#EXTM3U\n"), None);
    }

    const MEDIA: &str = "#EXTM3U
#EXT-X-VERSION:3
#EXT-X-PLAYLIST-TYPE:VOD
#EXT-X-TARGETDURATION:8
#EXTINF:5.0,
https://rr1---sn-a.googlevideo.com/videoplayback/sq/0/x.ts
#EXTINF:5.0,
https://rr1---sn-a.googlevideo.com/videoplayback/sq/1/x.ts
#EXTINF:6.5,
https://rr1---sn-a.googlevideo.com/videoplayback/sq/2/x.ts
#EXTINF:3.5,
https://rr1---sn-a.googlevideo.com/videoplayback/sq/3/x.ts
#EXT-X-ENDLIST
";

    fn segments(cut: &Cut) -> Vec<&str> {
        cut.segments.iter().map(|(_, url)| url.as_str()).collect()
    }

    #[test]
    fn clips_start_with_the_segment_they_show() {
        // 8% of 20 s is 1.6 s, in the first segment.
        let short = trim(MEDIA).unwrap();
        assert_eq!(segments(&short).len(), 4);
        assert_eq!(short.init, None);
        // 8% of 100 s is 8 s, in the second segment (5 to 10 s).
        let long = MEDIA.replace("6.5,", "10.0,").replace("3.5,", "80.0,");
        let long = trim(&long).unwrap();
        assert_eq!(
            segments(&long),
            [
                "https://rr1---sn-a.googlevideo.com/videoplayback/sq/1/x.ts",
                "https://rr1---sn-a.googlevideo.com/videoplayback/sq/2/x.ts",
                "https://rr1---sn-a.googlevideo.com/videoplayback/sq/3/x.ts",
            ]
        );
        assert_eq!(long.segments[0].0, 5.0);
    }

    #[test]
    fn fragmented_streams_keep_their_header() {
        let header = "#EXT-X-MAP:URI=\"https://rr1---sn-a.googlevideo.com/videoplayback/init\"\n";
        let vp9 = MEDIA.replace(
            "#EXTINF:5.0,\nhttps://rr1---sn-a.googlevideo.com/videoplayback/sq/0",
            &format!("{header}#EXTINF:5.0,\nhttps://rr1---sn-a.googlevideo.com/videoplayback/sq/0"),
        );
        let cut = trim(&vp9).unwrap();
        assert_eq!(
            cut.init.as_ref().map(Url::as_str),
            Some("https://rr1---sn-a.googlevideo.com/videoplayback/init")
        );
        let ranged = vp9.replace("/init\"", "/init\",BYTERANGE=\"720@0\"");
        let elsewhere = vp9.replace(
            "rr1---sn-a.googlevideo.com/videoplayback/init",
            "evil.example/init",
        );
        let twice = vp9.replace("#EXT-X-VERSION:3", &format!("#EXT-X-VERSION:3\n{header}"));
        for media in [ranged, elsewhere, twice] {
            assert_eq!(trim(&media), None, "{media}");
        }
    }

    #[test]
    fn only_plain_youtube_playlists_are_trimmed() {
        let elsewhere = MEDIA.replace("rr1---sn-a.googlevideo.com", "evil.example");
        let lookalike = MEDIA.replace("rr1---sn-a.googlevideo.com", "googlevideo.com.evil.example");
        let plain = MEDIA.replace("https://rr1", "http://rr1");
        let local = MEDIA.replace("https://rr1---sn-a.googlevideo.com", "file://");
        let keyed = MEDIA.replace("#EXT-X-VERSION:3", "#EXT-X-KEY:METHOD=AES-128,URI=\"x\"");
        let unfinished = MEDIA.replace("#EXT-X-ENDLIST", "");
        let untimed = MEDIA.replacen("#EXTINF:5.0,", "", 1);
        for media in [
            elsewhere,
            lookalike,
            plain,
            local,
            keyed,
            unfinished,
            untimed,
            String::new(),
        ] {
            assert_eq!(trim(&media), None, "{media}");
        }
    }

    /// The relay serves a clip's playlist under its own path and nothing else.
    #[tokio::test]
    async fn the_relay_serves_only_its_clips() {
        let relay = relay::Relay::start().await.unwrap();
        let cut = trim(MEDIA).unwrap();
        let link = Url::parse(&relay.add(None, &cut.segments[2..]).unwrap()).unwrap();
        assert_eq!(link.host_str(), Some("127.0.0.1"));
        let http = reqwest::Client::new();
        let playlist = http.get(link.clone()).send().await.unwrap();
        assert_eq!(playlist.status(), 200);
        assert_eq!(
            playlist.text().await.unwrap(),
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-TARGETDURATION:7\n\
             #EXTINF:6.5,\n0.ts\n#EXTINF:3.5,\n1.ts\n#EXT-X-ENDLIST\n"
        );
        let header = Url::parse("https://rr1---sn-a.googlevideo.com/videoplayback/init").unwrap();
        let fragmented = Url::parse(&relay.add(Some(header), &cut.segments[3..]).unwrap()).unwrap();
        assert!(
            http.get(fragmented)
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
                .contains(
                    "#EXT-X-VERSION:6\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-TARGETDURATION:4\n\
                 #EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:3.5,\n0.m4s\n"
                )
        );
        let mut other = link.clone();
        other.set_path("/0123456789abcdef0123456789abcdef/index.m3u8");
        let mut missing = link.clone();
        missing.set_path(&link.path().replace("index.m3u8", "9.ts"));
        let mut headless = link.clone();
        headless.set_path(&link.path().replace("index.m3u8", "init.mp4"));
        for url in [other, missing, headless] {
            assert_eq!(http.get(url).send().await.unwrap().status(), 404);
        }
        assert_eq!(http.post(link).send().await.unwrap().status(), 405);
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
        let clip = client.clip("movie", "tt1375666").await.unwrap();
        eprintln!("{:?} {}", started.elapsed(), clip.width);
        assert!(clip.width >= 1920 && clip.width <= MAX_STREAM_WIDTH);
        // The first segment comes through the relay, fetched from YouTube.
        let base = Url::parse(&clip.playlist).unwrap();
        let playlist = reqwest::get(base.clone())
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let first = playlist
            .lines()
            .find(|line| line.starts_with("0."))
            .expect("a first segment");
        for file in ["init.mp4", first] {
            let wanted = playlist.contains(file) || file == first;
            if !wanted {
                continue;
            }
            let response = reqwest::get(base.join(file).unwrap()).await.unwrap();
            assert_eq!(response.status(), 200, "{file}");
            assert!(response.bytes().await.unwrap().len() > 500, "{file}");
        }
    }
}
