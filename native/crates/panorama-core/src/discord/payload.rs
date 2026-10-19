use std::{error::Error, fmt};

use serde_json::{Map, Value, json};
use url::Url;

/// Whether the player is advancing; only `Playing` produces timestamps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackState {
    /// Frames are advancing.
    Playing,
    /// Paused by the user.
    Paused,
    /// Waiting for data.
    Buffering,
}

/// What the player reports; mirrors `DiscordPlayback` in the TS shared module.
#[derive(Clone, Debug, PartialEq)]
pub struct DiscordPlayback {
    /// Opaque film ID; `tmdb:<n>` IDs link to The Movie Database.
    pub film_id: String,
    /// Trimmed, non-empty title.
    pub title: String,
    /// Four-digit release year.
    pub year: Option<String>,
    /// TMDB person ID of the director, for the director link.
    pub director_tmdb_id: Option<u64>,
    /// Director name.
    pub director: Option<String>,
    /// `image.tmdb.org` artwork URL.
    pub artwork: Option<String>,
    /// Position in seconds.
    pub time: f64,
    /// Duration in seconds.
    pub duration: f64,
    /// Player state.
    pub state: PlaybackState,
}

/// The playback payload failed validation; carries no input data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPlayback;

impl fmt::Display for InvalidPlayback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid Discord playback")
    }
}

impl Error for InvalidPlayback {}

/// JS `String.length` (UTF-16 units), so limits match the TS validation.
fn js_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn take(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

/// First URL that is plain HTTPS on `image.tmdb.org` under `/t/p/`, as its href.
pub fn discord_artwork(urls: &[Option<&str>]) -> Option<String> {
    urls.iter().flatten().find_map(|value| {
        if value.is_empty() || js_len(value) > 2048 {
            return None;
        }
        let url = Url::parse(value).ok()?;
        (url.scheme() == "https"
            && url.host_str() == Some("image.tmdb.org")
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path().starts_with("/t/p/")
            && url.query().is_none()
            && url.fragment().is_none())
        .then(|| url.to_string())
    })
}

/// The `SET_ACTIVITY` activity for `playback` at wall-clock `now_ms`.
/// Text is cut to 128 Unicode scalar values (TS cuts UTF-16 units).
pub fn discord_activity(playback: &DiscordPlayback, now_ms: f64) -> Value {
    let start = (now_ms / 1000.0 - playback.time).floor() as i64;
    let details = match &playback.year {
        Some(year) if !year.is_empty() => format!("{} ({year})", take(&playback.title, 121)),
        _ => take(&playback.title, 128),
    };
    let tmdb_id = playback.film_id.strip_prefix("tmdb:").filter(|id| {
        !id.starts_with('0') && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
    });
    let mut fields = Map::new();
    fields.insert("type".into(), json!(3));
    fields.insert("name".into(), json!("in Panorama"));
    fields.insert("details".into(), json!(details));
    if let Some(id) = tmdb_id {
        fields.insert(
            "details_url".into(),
            json!(format!("https://www.themoviedb.org/movie/{id}")),
        );
    }
    fields.insert("status_display_type".into(), json!(2));
    if let Some(director) = playback.director.as_deref().filter(|d| !d.is_empty()) {
        fields.insert(
            "state".into(),
            json!(take(&format!("dir. {director}"), 128)),
        );
        if let Some(id) = playback.director_tmdb_id {
            fields.insert(
                "state_url".into(),
                json!(format!("https://www.themoviedb.org/person/{id}")),
            );
        }
    }
    if let Some(artwork) = &playback.artwork {
        fields.insert(
            "assets".into(),
            json!({"large_image": artwork, "large_text": details}),
        );
    }
    if playback.state == PlaybackState::Playing {
        let mut timestamps = Map::new();
        timestamps.insert("start".into(), json!(start));
        if playback.duration > playback.time {
            timestamps.insert("end".into(), json!(start + playback.duration.ceil() as i64));
        }
        fields.insert("timestamps".into(), Value::Object(timestamps));
    }
    Value::Object(fields)
}

fn optional_string(
    input: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, InvalidPlayback> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(InvalidPlayback),
    }
}

fn number(input: &Map<String, Value>, key: &str) -> Result<f64, InvalidPlayback> {
    input
        .get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && (0.0..=1e8).contains(n))
        .ok_or(InvalidPlayback)
}

/// Strictly validates an untrusted payload (`null` clears the presence). Unknown
/// keys, wrong types, out-of-range numbers and non-TMDB artwork are rejected.
pub fn parse_discord_playback(value: &Value) -> Result<Option<DiscordPlayback>, InvalidPlayback> {
    if value.is_null() {
        return Ok(None);
    }
    let input = value.as_object().ok_or(InvalidPlayback)?;
    const KEYS: [&str; 9] = [
        "filmId",
        "title",
        "year",
        "directorTmdbId",
        "director",
        "artwork",
        "time",
        "duration",
        "state",
    ];
    const REQUIRED: [&str; 7] = [
        "filmId", "title", "director", "artwork", "time", "duration", "state",
    ];
    if input.keys().any(|key| !KEYS.contains(&key.as_str()))
        || REQUIRED.iter().any(|key| !input.contains_key(*key))
    {
        return Err(InvalidPlayback);
    }
    let film_id = optional_string(input, "filmId")?
        .filter(|id| !id.is_empty() && js_len(id) <= 128)
        .ok_or(InvalidPlayback)?;
    let title = optional_string(input, "title")?
        .filter(|t| js_len(t) <= 512 && !t.trim().is_empty())
        .map(|t| t.trim().to_owned())
        .ok_or(InvalidPlayback)?;
    let year = optional_string(input, "year")?;
    if year
        .as_deref()
        .is_some_and(|y| y.len() != 4 || !y.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(InvalidPlayback);
    }
    let director_tmdb_id = match input.get("directorTmdbId") {
        None | Some(Value::Null) => None,
        Some(id) => Some(
            id.as_u64()
                .filter(|id| (1..(1 << 53)).contains(id))
                .ok_or(InvalidPlayback)?,
        ),
    };
    let director = optional_string(input, "director")?;
    if director
        .as_deref()
        .is_some_and(|d| js_len(d) > 512 || d.trim().is_empty())
    {
        return Err(InvalidPlayback);
    }
    let artwork = optional_string(input, "artwork")?;
    if artwork
        .as_deref()
        .is_some_and(|a| discord_artwork(&[Some(a)]).is_none())
    {
        return Err(InvalidPlayback);
    }
    let state = match input.get("state").and_then(Value::as_str) {
        Some("playing") => PlaybackState::Playing,
        Some("paused") => PlaybackState::Paused,
        Some("buffering") => PlaybackState::Buffering,
        _ => return Err(InvalidPlayback),
    };
    Ok(Some(DiscordPlayback {
        film_id,
        title,
        year,
        director_tmdb_id,
        director: director.map(|d| d.trim().to_owned()),
        artwork,
        time: number(input, "time")?,
        duration: number(input, "duration")?,
        state,
    }))
}
