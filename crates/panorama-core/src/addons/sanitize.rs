use stremio_core::types::{addon::ResourceResponse, resource::MetaItemPreview};
use url::Url;

use super::{FailureKind, FilmDetails, StreamSource, TransportResponse};

pub(super) const BODY_LIMIT: usize = 8 * 1024 * 1024;

pub(super) fn valid_id(id: &str) -> bool {
    let length = id.chars().count();
    (1..=200).contains(&length)
        && !id
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '?' | '#'))
}

pub(super) fn text(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

/// One origin country per film: addons send lists ("France, Belgium") and long official names.
pub(super) fn country(value: &str) -> Option<String> {
    let first = value
        .split([',', '/', '&', ';', '|'])
        .map(str::trim)
        .find(|part| !part.is_empty())?;
    let name = match first {
        "United States of America" | "USA" | "U.S.A." | "US" | "U.S." => "United States",
        "United Kingdom of Great Britain and Northern Ireland" | "UK" | "U.K." => "United Kingdom",
        other => other,
    };
    Some(text(name, 300))
}

fn optional(value: Option<String>, limit: usize) -> Option<String> {
    value.map(|value| text(&value, limit))
}

fn image(value: Option<Url>) -> Option<Url> {
    value.filter(|url| url.scheme() == "https" && url.as_str().len() <= 4096)
}

#[derive(Default)]
pub(super) struct Credits {
    pub director: Vec<String>,
    pub cast: Vec<String>,
    /// Cast and crew with photos, from a TMDB-backed addon's `app_extras`.
    pub people: Vec<crate::credits::Person>,
    pub previews: Vec<super::PreviewCredits>,
}

pub(super) struct Response {
    pub core: ResourceResponse,
    pub credits: Credits,
}

pub(super) fn credits(value: &serde_json::Value) -> Credits {
    let names = |field: &str| match value.get("meta").and_then(|meta| meta.get(field)) {
        Some(serde_json::Value::String(name)) => vec![text(name, 300)],
        Some(serde_json::Value::Array(names)) => names
            .iter()
            .filter_map(|name| name.as_str())
            .take(100)
            .map(|name| text(name, 300))
            .collect(),
        _ => vec![],
    };
    Credits {
        director: names("director"),
        cast: names("cast"),
        people: value
            .get("meta")
            .and_then(|meta| meta.get("app_extras"))
            .map(crate::credits::people_from_extras)
            .unwrap_or_default(),
        previews: value
            .get("metas")
            .and_then(|v| v.as_array())
            .map(|items| items.iter().take(200).collect::<Vec<_>>())
            .unwrap_or_else(|| value.get("meta").into_iter().collect())
            .into_iter()
            .filter_map(|item| {
                let id = item.get("id")?.as_str()?;
                if !valid_id(id) {
                    return None;
                }
                let director = match item.get("director") {
                    Some(serde_json::Value::String(name)) => vec![text(name, 300)],
                    Some(serde_json::Value::Array(names)) => names
                        .iter()
                        .filter_map(|v| v.as_str())
                        .take(100)
                        .map(|v| text(v, 300))
                        .collect(),
                    _ => vec![],
                };
                let country = item
                    .get("country")
                    .or_else(|| item.get("originCountry"))
                    .and_then(|v| v.as_str())
                    .and_then(country);
                // ponytail: field names used by TMDB-backed addons; extend when another appears.
                let original_name = ["originalName", "original_title", "originalTitle"]
                    .into_iter()
                    .find_map(|field| item.get(field)?.as_str())
                    .map(|name| text(name.trim(), 300))
                    .filter(|name| !name.is_empty());
                Some(super::PreviewCredits {
                    id: id.into(),
                    director,
                    country,
                    original_name,
                })
            })
            .collect(),
    }
}

pub(super) fn parse(response: TransportResponse) -> Result<Response, FailureKind> {
    let (core, credits) = match response {
        TransportResponse::Bytes(bytes) if bytes.len() > BODY_LIMIT => Err(FailureKind::TooLarge),
        TransportResponse::Bytes(bytes) => {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| FailureKind::InvalidResponse)?;
            let credits = credits(&value);
            let core = serde_json::from_value(value).map_err(|_| FailureKind::InvalidResponse)?;
            Ok((core, credits))
        }
        TransportResponse::Core(response) => {
            if serde_json::to_vec(&response)
                .map_err(|_| FailureKind::InvalidResponse)?
                .len()
                > BODY_LIMIT
            {
                return Err(FailureKind::TooLarge);
            }
            Ok((response, Credits::default()))
        }
        TransportResponse::WithCredits {
            response,
            director,
            cast,
            people,
            previews,
        } => {
            let mut parsed = parse(TransportResponse::Core(response))?;
            parsed.credits = Credits {
                director: director
                    .iter()
                    .take(100)
                    .map(|name| text(name, 300))
                    .collect(),
                cast: cast.iter().take(100).map(|name| text(name, 300)).collect(),
                people,
                previews: previews
                    .into_iter()
                    .take(200)
                    .filter(|p| valid_id(&p.id))
                    .map(|p| super::PreviewCredits {
                        id: p.id,
                        director: p.director.iter().take(100).map(|v| text(v, 300)).collect(),
                        country: p.country.as_deref().and_then(country),
                        original_name: p.original_name.as_deref().map(|v| text(v, 300)),
                    })
                    .collect(),
            };
            return Ok(parsed);
        }
    }?;
    Ok(Response { core, credits })
}

pub(super) fn film(preview: MetaItemPreview, thin: bool) -> Option<FilmDetails> {
    let name = text(&preview.name, 300);
    if !valid_id(&preview.id) || preview.r#type != "movie" || (!thin && name.trim().is_empty()) {
        return None;
    }
    let category = |name: &str| {
        preview
            .links
            .iter()
            .filter(|link| link.category.eq_ignore_ascii_case(name))
            .take(100)
            .map(|link| text(&link.name, 300))
            .collect::<Vec<_>>()
    };
    let genres = category("Genres");
    let director = category("Director");
    let cast = category("Cast");
    let imdb_rating = category("imdb").into_iter().next();
    let links: Vec<_> = preview
        .links
        .into_iter()
        .filter(|link| {
            matches!(link.url.scheme(), "http" | "https") && link.url.as_str().len() <= 4096
        })
        .take(100)
        .map(|mut link| {
            link.name = text(&link.name, 300);
            link.category = text(&link.category, 300);
            link
        })
        .collect();
    Some(FilmDetails {
        id: preview.id,
        name,
        released: preview.released.map(|date| date.format("%F").to_string()),
        release_info: optional(
            preview
                .release_info
                .or_else(|| preview.released.map(|date| date.format("%Y").to_string())),
            100,
        ),
        runtime: optional(preview.runtime, 100),
        genres,
        description: optional(preview.description, 5000),
        director,
        origin_country: category_country(&links),
        original_name: None,
        cast,
        people: vec![],
        poster: image(preview.poster),
        background: image(preview.background),
        logo: image(preview.logo),
        imdb_rating,
        links,
    })
}

pub(super) fn films(response: Response, thin: bool) -> Result<Vec<FilmDetails>, FailureKind> {
    let previews = match response.core {
        ResourceResponse::Metas { metas } => metas,
        ResourceResponse::MetasDetailed { metas_detailed } => metas_detailed
            .into_iter()
            .map(|item| item.preview)
            .collect(),
        _ => return Err(FailureKind::InvalidResponse),
    };
    Ok(previews
        .into_iter()
        .filter_map(|preview| film(preview, thin))
        .map(|mut film| {
            if let Some(credits) = response.credits.previews.iter().find(|p| p.id == film.id) {
                if !credits.director.is_empty() {
                    film.director.clone_from(&credits.director);
                }
                if credits.country.is_some() {
                    film.origin_country.clone_from(&credits.country);
                }
                if credits.original_name.is_some() {
                    film.original_name.clone_from(&credits.original_name);
                }
            }
            film
        })
        .take(200)
        .collect())
}

fn category_country(links: &[stremio_core::types::resource::Link]) -> Option<String> {
    links
        .iter()
        .find(|link| link.category.eq_ignore_ascii_case("country"))
        .and_then(|link| country(&link.name))
}

/// At most this many subtitle files from one addon or one stream.
const SUBTITLE_LIMIT: usize = 100;

fn subtitle(
    item: stremio_core::types::resource::Subtitles,
    addon: &str,
) -> Option<super::SubtitleSource> {
    (matches!(item.url.scheme(), "https" | "http") && item.url.as_str().len() <= 4096).then(|| {
        super::SubtitleSource {
            url: item.url,
            lang: text(item.lang.trim(), 50),
            addon: text(addon, 300),
        }
    })
}

pub(super) fn subtitles(
    response: Response,
    addon: &str,
) -> Result<Vec<super::SubtitleSource>, FailureKind> {
    let ResourceResponse::Subtitles { subtitles } = response.core else {
        return Err(FailureKind::InvalidResponse);
    };
    Ok(subtitles
        .into_iter()
        .filter_map(|item| subtitle(item, addon))
        .take(SUBTITLE_LIMIT)
        .collect())
}

impl StreamSource {
    /// Subtitle files the stream carries itself, credited to `addon`.
    pub fn subtitles(&self, addon: &str) -> Vec<super::SubtitleSource> {
        self.stream
            .subtitles
            .iter()
            .cloned()
            .filter_map(|item| subtitle(item, addon))
            .take(SUBTITLE_LIMIT)
            .collect()
    }
}

pub(super) fn sources(response: Response) -> Result<Vec<StreamSource>, FailureKind> {
    let ResourceResponse::Streams { streams } = response.core else {
        return Err(FailureKind::InvalidResponse);
    };
    Ok(streams
        .into_iter()
        .take(200)
        .map(|mut stream| {
            stream.name = optional(stream.name, 300);
            stream.description = optional(stream.description, 5000);
            stream.thumbnail = stream.thumbnail.filter(|value| {
                Url::parse(value)
                    .ok()
                    .and_then(|url| image(Some(url)))
                    .is_some()
            });
            StreamSource {
                name: stream.name.clone(),
                title: stream.description.clone(),
                description: stream.description.clone(),
                stream,
            }
        })
        .collect())
}
