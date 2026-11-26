//! A film's English and foreign titles straight from TMDB, with the user's own API key. Addons
//! send one localized name and rarely the original, so the film page asks TMDB for both.
use crate::{
    addons::{Episode, Kind},
    credits::text,
};
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
    /// A series' main directors; empty for films, whose addons already name theirs.
    #[serde(default)]
    pub directors: Vec<String>,
}

/// One episode as TMDB lists it, matched to an addon's by season and number.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TmdbEpisode {
    pub season: u32,
    pub number: u32,
    pub title: Option<String>,
    pub overview: Option<String>,
    /// The full-size still; the image cache sizes it to its card.
    pub still: Option<String>,
    pub air_date: Option<String>,
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

    /// TMDB's own id for an IMDb or `tmdb:` film id; `None` when TMDB does not know it.
    async fn tmdb_id(
        &self,
        key: &str,
        kind: Kind,
        film_id: &str,
    ) -> Result<Option<u64>, TmdbError> {
        if let Some(id) = film_id.strip_prefix("tmdb:") {
            return id.parse().map(Some).map_err(|_| TmdbError::Invalid);
        }
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
        Ok(found[results][0]["id"].as_u64())
    }

    /// A series' episodes in `seasons`, as TMDB lists them; `None` when TMDB does not know it.
    pub async fn episodes(
        &self,
        key: &str,
        film_id: &str,
        seasons: &[u32],
    ) -> Result<Option<Vec<TmdbEpisode>>, TmdbError> {
        let Some(id) = self.tmdb_id(key, Kind::Series, film_id).await? else {
            return Ok(None);
        };
        let mut episodes = vec![];
        // TMDB appends at most twenty seasons to one request.
        for chunk in seasons.chunks(20) {
            let append = chunk
                .iter()
                .map(|season| format!("season/{season}"))
                .collect::<Vec<_>>()
                .join(",");
            let details = self
                .get(
                    key,
                    &format!("/tv/{id}"),
                    &[("language", "en-US"), ("append_to_response", &append)],
                )
                .await?;
            episodes.extend(episodes_from_details(&details));
        }
        Ok(Some(episodes))
    }

    /// A film's titles; `None` when TMDB does not know it.
    pub async fn titles(
        &self,
        key: &str,
        kind: Kind,
        film_id: &str,
    ) -> Result<Option<Titles>, TmdbError> {
        let path = if kind == Kind::Movie { "movie" } else { "tv" };
        let Some(id) = self.tmdb_id(key, kind, film_id).await? else {
            return Ok(None);
        };
        let details = self
            .get(
                key,
                &format!("/{path}/{id}"),
                &[
                    ("language", "en-US"),
                    (
                        "append_to_response",
                        if kind == Kind::Movie {
                            "alternative_titles"
                        } else {
                            "alternative_titles,aggregate_credits,credits"
                        },
                    ),
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

/// Every episode of the `season/N` responses appended to a show's details.
pub fn episodes_from_details(details: &Value) -> Vec<TmdbEpisode> {
    let Some(details) = details.as_object() else {
        return vec![];
    };
    details
        .iter()
        .filter(|(key, _)| key.starts_with("season/"))
        .flat_map(|(_, season)| season["episodes"].as_array().into_iter().flatten())
        .filter_map(|episode| {
            Some(TmdbEpisode {
                season: u32::try_from(episode["season_number"].as_u64()?).ok()?,
                number: u32::try_from(episode["episode_number"].as_u64()?).ok()?,
                title: text(&episode["name"]),
                // Whole: `text` stops at 200 characters, which a synopsis often passes.
                overview: episode["overview"]
                    .as_str()
                    .map(str::trim)
                    .filter(|overview| !overview.is_empty())
                    .map(str::to_owned),
                still: episode["still_path"]
                    .as_str()
                    .filter(|path| path.starts_with('/'))
                    .map(|path| format!("https://image.tmdb.org/t/p/original{path}")),
                air_date: text(&episode["air_date"]),
            })
        })
        .collect()
}

/// Lay TMDB's stills and synopses over an addon's episodes, matched by season and number. The
/// addon's ids stay, since its streams are found by them, and so do its titles, which are often
/// typeset better; TMDB names only an episode the addon left blank or called "Episode 5".
pub fn enrich(episodes: &mut [Episode], tmdb: &[TmdbEpisode]) {
    for episode in episodes.iter_mut().filter(|episode| episode.numbered) {
        let Some(found) = tmdb
            .iter()
            .find(|found| (found.season, found.number) == (episode.season, episode.number))
        else {
            continue;
        };
        if let Some(still) = found.still.as_deref().and_then(|url| url.parse().ok()) {
            episode.thumbnail = Some(still);
        }
        if let Some(overview) = &found.overview {
            episode.overview = Some(overview.clone());
        }
        let stand_in =
            |title: &str| title.is_empty() || title == format!("Episode {}", episode.number);
        if stand_in(episode.title.trim())
            && let Some(title) = found.title.as_ref().filter(|title| !stand_in(title))
        {
            episode.title = title.clone();
        }
        if episode.released.is_none() {
            episode.released = found.air_date.clone();
        }
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
    Some(Titles {
        english,
        original,
        directors: series_directors(details),
    })
}

/// Everyone who directed an episode of a series, most episodes first, as Letterboxd lists
/// them; for a show whose episodes carry no credits yet, its own crew's directors.
fn series_directors(details: &Value) -> Vec<String> {
    let crew = |credits: &str| details[credits]["crew"].as_array().into_iter().flatten();
    let mut directors: Vec<(u64, String)> = crew("aggregate_credits")
        .filter_map(|member| {
            let episodes: u64 = member["jobs"]
                .as_array()?
                .iter()
                .filter(|job| job["job"].as_str() == Some("Director"))
                .map(|job| job["episode_count"].as_u64().unwrap_or(1))
                .sum();
            (episodes > 0)
                .then(|| text(&member["name"]))?
                .map(|name| (episodes, name))
        })
        .collect();
    if directors.is_empty() {
        directors = crew("credits")
            .filter(|member| member["job"].as_str() == Some("Director"))
            .filter_map(|member| Some((1, text(&member["name"])?)))
            .collect();
    }
    // Stable, so directors with as many episodes keep TMDB's order.
    directors.sort_by_key(|(episodes, _)| std::cmp::Reverse(*episodes));
    let mut names: Vec<String> = vec![];
    for (_, name) in directors {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    // ponytail: four fit a card and the byline; a long-running show's full list would not.
    names.truncate(4);
    names
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
    fn tmdb_episodes_dress_the_addons_and_keep_their_ids() {
        let details = json!({
            "name": "Silo",
            "season/1": {"episodes": [
                {"season_number": 1, "episode_number": 1, "name": "Freedom Day",
                 "overview": "Sheriff Becker's plans...", "still_path": "/a.jpg",
                 "air_date": "2023-05-05"},
                {"season_number": 1, "episode_number": 2, "name": "Episode 2",
                 "overview": "", "still_path": null, "air_date": "2023-05-05"},
            ]},
        });
        let tmdb = episodes_from_details(&details);
        assert_eq!(tmdb.len(), 2);
        assert_eq!(
            tmdb[0].still.as_deref(),
            Some("https://image.tmdb.org/t/p/original/a.jpg")
        );
        let addon = |number: u32, title: &str| Episode {
            id: format!("tt14688458:1:{number}"),
            season: 1,
            number,
            numbered: true,
            title: title.into(),
            overview: Some("Cinemeta's".into()),
            thumbnail: None,
            released: None,
        };
        let mut episodes = vec![
            addon(1, "Episode 1"),
            addon(2, "Holston’s Pick"),
            addon(3, "Machines"),
        ];
        enrich(&mut episodes, &tmdb);
        assert_eq!(episodes[0].id, "tt14688458:1:1");
        assert_eq!(episodes[0].title, "Freedom Day", "TMDB names a stand-in");
        assert_eq!(
            episodes[0].thumbnail.as_ref().map(|url| url.as_str()),
            Some("https://image.tmdb.org/t/p/original/a.jpg")
        );
        assert_eq!(
            episodes[0].overview.as_deref(),
            Some("Sheriff Becker's plans...")
        );
        assert_eq!(episodes[0].released.as_deref(), Some("2023-05-05"));
        assert_eq!(
            episodes[1].title, "Holston’s Pick",
            "the addon's own title stays"
        );
        assert_eq!(
            episodes[1].overview.as_deref(),
            Some("Cinemeta's"),
            "an empty synopsis neither"
        );
        assert_eq!(episodes[1].thumbnail, None);
        assert_eq!(
            episodes[2].overview.as_deref(),
            Some("Cinemeta's"),
            "TMDB lacks it"
        );
    }

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
                original: Some("Le Mépris".into()),
                directors: vec![],
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
    fn a_series_lists_its_episode_directors_most_episodes_first() {
        let director = |name: &str, episodes: u64| {
            json!({ "name": name, "jobs": [
                { "job": "Director", "episode_count": episodes },
                { "job": "Writer", "episode_count": 9 },
            ]})
        };
        let show = |crew: Value| json!({ "name": "Show", "aggregate_credits": { "crew": crew } });
        let rotating = show(json!([
            director("A", 1),
            director("B", 3),
            director("C", 1),
            { "name": "Writer", "jobs": [{ "job": "Writer", "episode_count": 8 }] },
            director("D", 1),
            director("E", 1),
        ]));
        assert_eq!(
            titles_from_details(&rotating).unwrap().directors,
            ["B", "A", "C", "D"]
        );
        let unaired = json!({ "name": "Show", "aggregate_credits": { "crew": [] },
        "credits": { "crew": [
            { "name": "Christopher J. Byrne", "job": "Director" },
            { "name": "Terry Matalas", "job": "Executive Producer" },
        ]}});
        assert_eq!(
            titles_from_details(&unaired).unwrap().directors,
            ["Christopher J. Byrne"]
        );
        let film = json!({ "title": "Heat" });
        assert!(titles_from_details(&film).unwrap().directors.is_empty());
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
