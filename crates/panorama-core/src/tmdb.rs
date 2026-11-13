//! A film's English and foreign titles straight from TMDB, with the user's own API key. Addons
//! send one localized name and rarely the original, so the film page asks TMDB for both.
use crate::{addons::Kind, credits::text};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://api.themoviedb.org/3";
const TIMEOUT: Duration = Duration::from_secs(10);
/// TMDB's types for a romanized title, best first.
const TRANSLITERATED: [&str; 3] = [
    "transliterated title",
    "transliterated title (alternative spelling)",
    "alternative transliterated title",
];

/// The title shown on top and the one set under it.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Titles {
    pub english: String,
    /// The original title, romanized when TMDB has a transliteration; `None` for an English
    /// film or one whose original matches the English title.
    pub original: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TmdbError {
    /// TMDB refused the key.
    Rejected,
    Network,
    Invalid,
}

#[derive(Clone)]
pub struct TmdbClient {
    http: reqwest::Client,
}

impl TmdbClient {
    pub fn new() -> Result<Self, TmdbError> {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .referer(false)
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| TmdbError::Network)?;
        Ok(Self { http })
    }

    /// Whether TMDB accepts the key.
    pub async fn check(&self, key: &str) -> Result<(), TmdbError> {
        self.get(key, "/configuration", &[]).await.map(|_| ())
    }

    /// A film's titles; `None` when TMDB does not know it.
    pub async fn titles(
        &self,
        key: &str,
        kind: Kind,
        film_id: &str,
    ) -> Result<Option<Titles>, TmdbError> {
        let path = if kind == Kind::Movie { "movie" } else { "tv" };
        let id = match film_id.strip_prefix("tmdb:") {
            Some(id) => id.parse::<u64>().map_err(|_| TmdbError::Invalid)?,
            None => {
                if !is_imdb(film_id) {
                    return Err(TmdbError::Invalid);
                }
                let found = self
                    .get(
                        key,
                        &format!("/find/{film_id}"),
                        &[("external_source", "imdb_id")],
                    )
                    .await?;
                let results = if kind == Kind::Movie {
                    "movie_results"
                } else {
                    "tv_results"
                };
                match found[results][0]["id"].as_u64() {
                    Some(id) => id,
                    None => return Ok(None),
                }
            }
        };
        let details = self
            .get(
                key,
                &format!("/{path}/{id}"),
                &[
                    ("language", "en-US"),
                    ("append_to_response", "alternative_titles"),
                ],
            )
            .await?;
        Ok(titles_from_details(&details))
    }

    /// A v3 key (32 hex characters) goes in the query; anything else is a v4 read token.
    async fn get(&self, key: &str, path: &str, query: &[(&str, &str)]) -> Result<Value, TmdbError> {
        let key = key.trim();
        let mut request = self.http.get(format!("{API}{path}")).query(query);
        request = if key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
            request.query(&[("api_key", key)])
        } else {
            request.bearer_auth(key)
        };
        let response = request.send().await.map_err(|_| TmdbError::Network)?;
        match response.status().as_u16() {
            401 => return Err(TmdbError::Rejected),
            404 => return Ok(Value::Null),
            status if !(200..300).contains(&status) => return Err(TmdbError::Network),
            _ => {}
        }
        response.json().await.map_err(|_| TmdbError::Invalid)
    }
}

fn is_imdb(id: &str) -> bool {
    id.strip_prefix("tt").is_some_and(|digits| {
        (1..=12).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

/// Letters only from the Latin blocks, so a romanized title can stand in for the original.
fn latin(title: &str) -> bool {
    title
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c <= '\u{024F}' || ('\u{1E00}'..='\u{1EFF}').contains(&c))
}

/// The English title on top; under it the original if Latin, else the origin country's
/// transliteration, else the original script.
pub fn titles_from_details(details: &Value) -> Option<Titles> {
    let english = text(&details["title"]).or_else(|| text(&details["name"]))?;
    let original = text(&details["original_title"]).or_else(|| text(&details["original_name"]));
    let original = original
        .filter(|_| details["original_language"].as_str() != Some("en"))
        .map(|original| {
            if latin(&original) {
                return original;
            }
            transliteration(details).unwrap_or(original)
        })
        .filter(|original| original.to_lowercase() != english.to_lowercase());
    Some(Titles { english, original })
}

fn transliteration(details: &Value) -> Option<String> {
    let alternatives = &details["alternative_titles"];
    let list = alternatives["titles"]
        .as_array()
        .or_else(|| alternatives["results"].as_array())?;
    let countries: Vec<&str> = details["origin_country"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .chain(
            details["production_countries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|country| country["iso_3166_1"].as_str()),
        )
        .collect();
    TRANSLITERATED.iter().find_map(|kind| {
        list.iter()
            .filter(|alt| {
                alt["type"]
                    .as_str()
                    .is_some_and(|t| t.eq_ignore_ascii_case(kind))
            })
            .filter(|alt| {
                alt["iso_3166_1"]
                    .as_str()
                    .is_some_and(|country| countries.contains(&country))
            })
            .filter_map(|alt| text(&alt["title"]))
            .find(|title| latin(title))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_latin_original_sits_under_the_english_title() {
        let le_mepris = json!({
            "title": "Contempt", "original_title": "Le Mépris", "original_language": "fr",
            "origin_country": ["FR"],
        });
        assert_eq!(
            titles_from_details(&le_mepris),
            Some(Titles {
                english: "Contempt".into(),
                original: Some("Le Mépris".into())
            })
        );
    }

    #[test]
    fn a_non_latin_original_uses_the_origin_countrys_transliteration() {
        let first_case = json!({
            "title": "First Case, Second Case", "original_title": "قضیه شکل اول، شکل دوم",
            "original_language": "fa", "origin_country": ["IR"],
            "alternative_titles": { "titles": [
                { "iso_3166_1": "US", "title": "First Case, Second Case", "type": "" },
                { "iso_3166_1": "IR", "title": "Ghazieh-e shekl-e aval", "type": "alternative transliterated title" },
                { "iso_3166_1": "IR", "title": "Ghazieh-e shekl-e aval, ghazieh-e shekl-e dou wom", "type": "transliterated title" },
            ]},
        });
        assert_eq!(
            titles_from_details(&first_case)
                .unwrap()
                .original
                .as_deref(),
            Some("Ghazieh-e shekl-e aval, ghazieh-e shekl-e dou wom")
        );
    }

    #[test]
    fn without_a_transliteration_the_original_script_stays() {
        let show = json!({
            "name": "Perfect Days", "original_name": "パーフェクト・デイズ",
            "original_language": "ja", "origin_country": ["JP"],
            "alternative_titles": { "results": [
                { "iso_3166_1": "DE", "title": "Perfekte Tage", "type": "transliterated title" },
            ]},
        });
        assert_eq!(
            titles_from_details(&show).unwrap().original.as_deref(),
            Some("パーフェクト・デイズ")
        );
    }

    #[test]
    fn english_films_and_matching_titles_get_no_second_line() {
        let english =
            json!({ "title": "Heat", "original_title": "Heat (1995)", "original_language": "en" });
        assert_eq!(titles_from_details(&english).unwrap().original, None);
        let same =
            json!({ "title": "Amélie", "original_title": "AMÉLIE", "original_language": "fr" });
        assert_eq!(titles_from_details(&same).unwrap().original, None);
        assert_eq!(titles_from_details(&Value::Null), None);
    }
}
