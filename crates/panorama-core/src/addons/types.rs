use chrono::{Datelike, Utc};
use std::{fmt, pin::Pin};

use futures::Stream;
use serde::{Deserialize, Serialize};
use stremio_core::types::resource::{Link, Stream as CoreStream};
use url::Url;

/// Local installation identity: manifest ID plus a salted transport hash.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct AddonKey(pub(super) String);

impl AddonKey {
    /// Returns the safe identifier used in preferences and cache keys.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a title is, as an addon types it. A catalog's kind and its items' kinds are
/// independent: Kitsu lists `anime` catalogs whose items are typed `series`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Movie,
    Series,
    Anime,
}

impl Kind {
    /// The type string addon requests carry.
    pub fn stremio(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Series => "series",
            Self::Anime => "anime",
        }
    }

    /// `None` for types Panorama has no screen for, such as `tv` or `channel`.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "movie" => Some(Self::Movie),
            "series" => Some(Self::Series),
            "anime" => Some(Self::Anime),
            _ => None,
        }
    }
}

/// A catalog belonging to one local addon installation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogRef {
    /// Local installation identity.
    pub addon: AddonKey,
    /// Opaque catalog ID.
    pub catalog_id: String,
    /// Display name.
    pub name: String,
    /// The catalog's own type; stored selections from before series support are movies.
    #[serde(default)]
    pub kind: Kind,
}

/// One episode of a series or anime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    /// The addon's `Video.id`, opaque: `tt0386676:5:1`, `kitsu:12:5`. Never composed.
    pub id: String,
    /// Season number; 0 is specials, which sort last.
    pub season: u32,
    /// Episode number within the season.
    pub number: u32,
    /// Whether the addon numbered it. The watched bitfield lists unnumbered videos first.
    #[serde(default)]
    pub numbered: bool,
    /// Display title.
    pub title: String,
    /// Synopsis.
    pub overview: Option<String>,
    /// HTTPS still.
    pub thumbnail: Option<Url>,
    /// `YYYY-MM-DD` air date; a later date is shown but not playable.
    pub released: Option<String>,
}

/// The episode after `video_id` in watch order, crossing seasons and skipping any released
/// after `today` (`YYYY-MM-DD`). Specials (season 0) are only followed by other specials.
pub fn next_after<'a>(episodes: &'a [Episode], video_id: &str, today: &str) -> Option<&'a Episode> {
    let mut ordered: Vec<_> = episodes.iter().collect();
    ordered.sort_by_key(|episode| (episode.season == 0, episode.season, episode.number));
    let current = ordered.iter().position(|episode| episode.id == video_id)?;
    let special = ordered[current].season == 0;
    ordered[current + 1..]
        .iter()
        .copied()
        .filter(|episode| (episode.season == 0) == special)
        .find(|episode| episode.released.as_deref().is_none_or(|date| date <= today))
}

/// Sanitized fields needed by film cards and the Film screen.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FilmDetails {
    /// Opaque addon film ID.
    pub id: String,
    /// The item's own type, which may differ from the type of the catalog that listed it.
    #[serde(default)]
    pub kind: Kind,
    /// Display name.
    pub name: String,
    /// Original-language title, when the addon supplies one.
    #[serde(default)]
    pub original_name: Option<String>,
    /// Year or release information supplied by the addon.
    pub release_info: Option<String>,
    /// `YYYY-MM-DD` release date, when the addon gives one.
    #[serde(default)]
    pub released: Option<String>,
    /// Display runtime supplied by the addon.
    pub runtime: Option<String>,
    /// Genre names.
    pub genres: Vec<String>,
    /// Plot description.
    pub description: Option<String>,
    /// Directors, when provided by metadata fields or links.
    pub director: Vec<String>,
    /// Origin country supplied by the addon, when available.
    #[serde(default)]
    pub origin_country: Option<String>,
    /// Cast, when provided by metadata fields or links.
    pub cast: Vec<String>,
    /// Cast and crew with photos, when a TMDB-backed addon sends them.
    #[serde(default)]
    pub people: Vec<crate::credits::Person>,
    /// HTTPS poster URL.
    pub poster: Option<Url>,
    /// HTTPS background URL.
    pub background: Option<Url>,
    /// HTTPS logo URL.
    pub logo: Option<Url>,
    /// IMDb rating supplied by the addon.
    pub imdb_rating: Option<String>,
    /// HTTP(S) links only.
    pub links: Vec<Link>,
    /// Episodes in watch order, specials last. Only details fetches fill this.
    #[serde(default)]
    pub episodes: Vec<Episode>,
}

impl FilmDetails {
    fn year(&self) -> Option<i32> {
        self.release_info.as_deref()?.get(..4)?.parse().ok()
    }
}

/// Keep what can be watched: an exact date decides, then a year, which only proves anything
/// once it is a later one. A film with neither goes too, unless nothing in the list has one,
/// since then the addon just doesn't send them. Festival pages skip this: a screened film
/// belongs there even while its general release is ahead.
pub fn only_released(films: &mut Vec<FilmDetails>) {
    let now = Utc::now();
    let today = now.format("%F").to_string();
    let dated = films
        .iter()
        .any(|film| film.released.is_some() || film.year().is_some());
    films.retain(|film| match (&film.released, film.year()) {
        (Some(date), _) => *date <= today,
        (None, Some(year)) => year <= now.year(),
        (None, None) => !dated,
    });
}

/// A catalog page and the installation that actually supplied it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Page {
    /// Source catalog; may differ from the requested catalog after fallback.
    pub catalog: CatalogRef,
    /// At most 200 usable cards.
    pub items: Vec<FilmDetails>,
    /// Raw source offset for the next request, before unusable cards are removed.
    #[serde(default)]
    pub next_skip: usize,
    /// Whether the source supports skip paging and returned a nonempty page.
    #[serde(default)]
    pub has_more: bool,
}

/// Display fields preserved before Stremio discards legacy preview properties.
#[derive(Clone, Default)]
pub struct PreviewCredits {
    /// Opaque movie identifier.
    pub id: String,
    /// Bounded director names.
    pub director: Vec<String>,
    /// Bounded origin country.
    pub country: Option<String>,
    /// Bounded original-language title.
    pub original_name: Option<String>,
}

/// Sanitized failures; never retain transport URLs, response bodies or raw errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    /// Network is unavailable; cached data may precede this event.
    Offline,
    /// A reachable source failed.
    Network,
    /// A source exceeded its deadline.
    Timeout,
    /// Invalid JSON, wrong resource shape or no usable catalog items.
    InvalidResponse,
    /// Response exceeds 8 MiB.
    TooLarge,
    /// Invalid film ID or installation identity.
    InvalidInput,
    /// Reading or writing storage failed.
    Storage,
}

impl fmt::Display for FailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FailureKind {}

/// Cache-first resource delivery, followed by a replacement or retryable error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceEvent<T> {
    /// Previously stored value, emitted before any request.
    CacheHit(T),
    /// Newly fetched replacement.
    Fresh(T),
    /// Sanitized terminal error; retry by calling again.
    Failed(FailureKind),
}

/// Home catalog events.
pub type CatalogStream = Pin<Box<dyn Stream<Item = ResourceEvent<Page>> + Send>>;
/// Film metadata events.
pub type DetailsStream = Pin<Box<dyn Stream<Item = ResourceEvent<FilmDetails>> + Send>>;
/// Search snapshots, including in-place metadata enrichment.
pub type SearchStream = Pin<Box<dyn Stream<Item = ResourceEvent<Vec<FilmDetails>>> + Send>>;

/// A playable core stream and bounded display fields. Debug output is redacted.
#[derive(Clone, Eq, PartialEq)]
pub struct StreamSource {
    /// Complete core playback source and behavior hints; may contain secrets.
    pub stream: CoreStream,
    /// Display name.
    pub name: Option<String>,
    /// Display title (the core aliases title to description).
    pub title: Option<String>,
    /// Display description.
    pub description: Option<String>,
}

impl fmt::Debug for StreamSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StreamSource { .. }")
    }
}

/// A subtitle file an addon offers, from its `subtitles` resource or inside a stream.
/// Debug output is redacted: the URL can carry addon credentials.
#[derive(Clone, Eq, PartialEq)]
pub struct SubtitleSource {
    /// An `http(s)` file URL.
    pub url: url::Url,
    /// Language as the addon names it, usually ISO 639-2 ("eng").
    pub lang: String,
    /// Display name of the addon that offered it.
    pub addon: String,
}

impl fmt::Debug for SubtitleSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SubtitleSource {{ lang: {:?}, .. }}", self.lang)
    }
}

/// One addon's isolated stream result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamState {
    /// At most 200 playable streams.
    Ready(Vec<StreamSource>),
    /// The addon recognizes the film but supplies no streams.
    Empty,
    /// This addon failed without discarding other groups.
    Failed(FailureKind),
}

/// Stream results belonging to a single installation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamGroup {
    /// Local installation identity.
    pub addon: AddonKey,
    /// Addon display name.
    pub name: String,
    /// Isolated result.
    pub state: StreamState,
}

/// Account-ordered stream results, separate from metadata delivery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamsEvent {
    /// No installed stream resource recognizes this film.
    NoSources,
    /// Results in account order, including isolated failures. Sent each time an addon answers
    /// with something playable, holding the addons heard from so far, and once all have.
    Groups(Vec<StreamGroup>),
    /// Invalid input or offline failure.
    Failed(FailureKind),
}

/// Uncached stream delivery; call again to retry expired links or failures.
pub type StreamsStream = Pin<Box<dyn Stream<Item = StreamsEvent> + Send>>;
