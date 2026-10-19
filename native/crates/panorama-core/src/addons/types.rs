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

/// A movie catalog belonging to one local addon installation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogRef {
    /// Local installation identity.
    pub addon: AddonKey,
    /// Opaque catalog ID.
    pub catalog_id: String,
    /// Display name.
    pub name: String,
}

/// Sanitized fields needed by film cards and the Film screen.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FilmDetails {
    /// Opaque addon film ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Year or release information supplied by the addon.
    pub release_info: Option<String>,
    /// Display runtime supplied by the addon.
    pub runtime: Option<String>,
    /// Genre names.
    pub genres: Vec<String>,
    /// Plot description.
    pub description: Option<String>,
    /// Directors, when provided by metadata fields or links.
    pub director: Vec<String>,
    /// Cast, when provided by metadata fields or links.
    pub cast: Vec<String>,
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
}

/// A catalog page and the installation that actually supplied it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Page {
    /// Source catalog; may differ from the requested catalog after fallback.
    pub catalog: CatalogRef,
    /// At most 200 usable movie cards.
    pub items: Vec<FilmDetails>,
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
    /// Results in account order, including isolated failures.
    Groups(Vec<StreamGroup>),
    /// Invalid input or offline failure.
    Failed(FailureKind),
}

/// Uncached stream delivery; call again to retry expired links or failures.
pub type StreamsStream = Pin<Box<dyn Stream<Item = StreamsEvent> + Send>>;
