//! Awards and festivals from MUBI's public web API: its id for a film, every festival entry
//! and prize the film has, and the festivals' own pages. Optional like the rest of the film
//! page's sections; callers fall back to Wikidata (`crate::credits`) when MUBI has nothing.
use crate::credits::{Award, AwardResult, CreditsError, Event, json, text};
use serde_json::Value;
use std::{collections::HashMap, time::Duration};
use url::Url;

const API: &str = "https://api.mubi.com/v4";
const SITE: &str = "https://mubi.com/en/awards-and-festivals";
/// MUBI refuses (406) a request without these headers. Its API and site accept Panorama's own
/// user agent, so the client names itself like the Wikidata one does.
const USER_AGENT: &str = concat!(
    "Panorama/",
    env!("CARGO_PKG_VERSION"),
    " (desktop film client)"
);
/// A festival page is ~110 KB of HTML around the data we want.
const MAX_PAGE: usize = 2 << 20;
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_AWARDS: usize = 400;
const MAX_FILMS: usize = 60;
/// An edition's entries, one per film; the largest festivals run to a few hundred.
const MAX_ENTRIES: usize = 2000;
/// Synopses are a paragraph; MUBI's short ones run to about 300 characters.
const MAX_SYNOPSIS: usize = 600;
const MAX_YEARS: usize = 200;
const MAX_SEARCH: usize = 10;
const MAX_URL: usize = 2048;
const MAX_QUERY_CHARS: usize = 200;
const IMAGE_HOSTS: [&str; 2] = ["images.mubicdn.net", "assets.mubicdn.net"];

/// The film to look for: what Stremio's metadata says about it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilmQuery<'a> {
    pub title: &'a str,
    pub original_title: Option<&'a str>,
    pub year: Option<i32>,
    pub directors: &'a [String],
}

/// A MUBI festival or award.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Festival {
    pub name: String,
    pub event: Event,
    /// Editions MUBI lists films for, newest first.
    pub years: Vec<i32>,
}

/// A film in a festival's lineup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FestivalFilm {
    /// MUBI's film id, which [`MubiClient::festival_results`] is keyed by.
    pub id: Option<u64>,
    pub title: String,
    pub year: Option<i32>,
    pub directors: Vec<String>,
    pub still: Option<Url>,
    /// MUBI's one-paragraph synopsis.
    pub synopsis: Option<String>,
    pub minutes: Option<u32>,
    pub genres: Vec<String>,
    /// The film's best result at this edition and its category; `None` for a plain entry.
    /// Filled in from [`MubiClient::festival_results`]: the films list only carries each
    /// film's best-known award anywhere, which is not this festival's.
    pub result: Option<(AwardResult, Option<String>)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FestivalPage {
    pub films: Vec<FestivalFilm>,
    pub next_page: Option<u32>,
}

/// MUBI over HTTPS, with bounded answers.
#[derive(Clone)]
pub struct MubiClient {
    http: reqwest::Client,
}

impl MubiClient {
    pub fn new() -> Result<Self, CreditsError> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Client", "web".parse().map_err(|_| CreditsError::Network)?);
        headers.insert(
            "Client-Country",
            "US".parse().map_err(|_| CreditsError::Network)?,
        );
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .referer(false)
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| CreditsError::Network)?;
        Ok(Self { http })
    }

    /// MUBI's id for the film, or `None` when no result is the same film.
    pub async fn find_film(&self, film: &FilmQuery<'_>) -> Result<Option<u64>, CreditsError> {
        let mut titles = vec![film.title];
        if let Some(original) = film.original_title
            && fold(original) != fold(film.title)
        {
            titles.push(original);
        }
        for title in titles {
            let title: String = title.trim().chars().take(MAX_QUERY_CHARS).collect();
            if title.is_empty() {
                continue;
            }
            let mut url = endpoint(&["search", "films"])?;
            url.query_pairs_mut()
                .append_pair("query", &title)
                .append_pair("per_page", &MAX_SEARCH.to_string());
            let found = search_from_json(&self.api(url).await?, film);
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }

    /// Every festival entry and prize of the film, in MUBI's order.
    pub async fn awards(&self, mubi_id: u64) -> Result<Vec<Award>, CreditsError> {
        let url = endpoint(&["films", &mubi_id.to_string(), "industry_event_entries"])?;
        Ok(awards_from_json(&self.api(url).await?))
    }

    /// The festival's identity and the years MUBI has a lineup for. The API object has no years,
    /// so they come from the festival's web page.
    pub async fn festival(&self, slug: &str) -> Result<Festival, CreditsError> {
        let slug = valid_slug(slug)?;
        let mut festival =
            festival_from_json(&self.api(endpoint(&["industry_events", slug])?).await?)
                .ok_or(CreditsError::Invalid)?;
        // Without its years the page cannot pick or switch an edition, so a failure here is
        // the festival's failure, retried with it.
        festival.years = self.years(slug).await?;
        Ok(festival)
    }

    /// One page of a festival edition's lineup.
    pub async fn festival_films(
        &self,
        event_id: u64,
        year: i32,
        page: u32,
    ) -> Result<FestivalPage, CreditsError> {
        let mut url = endpoint(&["industry_events", &event_id.to_string(), "films"])?;
        url.query_pairs_mut()
            .append_pair("year", &year.to_string())
            .append_pair("page", &page.max(1).to_string());
        Ok(films_from_json(&self.api(url).await?))
    }

    /// Each film's best result at one edition, by MUBI film id.
    pub async fn festival_results(
        &self,
        event_id: u64,
        year: i32,
    ) -> Result<HashMap<u64, (AwardResult, Option<String>)>, CreditsError> {
        let mut url = endpoint(&["industry_events", &event_id.to_string(), "film_entries"])?;
        url.query_pairs_mut().append_pair("year", &year.to_string());
        Ok(results_from_json(&self.api(url).await?, event_id))
    }

    async fn api(&self, url: Url) -> Result<Value, CreditsError> {
        json(
            self.http
                .get(url)
                .header(reqwest::header::ACCEPT, "application/json"),
        )
        .await
    }

    async fn years(&self, slug: &str) -> Result<Vec<i32>, CreditsError> {
        let mut url = Url::parse(SITE).map_err(|_| CreditsError::Invalid)?;
        url.path_segments_mut()
            .map_err(|_| CreditsError::Invalid)?
            .push(slug);
        let mut response = self
            .http
            .get(url)
            .header(reqwest::header::ACCEPT, "text/html")
            .send()
            .await
            .map_err(|_| CreditsError::Network)?
            .error_for_status()
            .map_err(|_| CreditsError::Network)?;
        let mut page = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| CreditsError::Network)? {
            page.extend_from_slice(&chunk);
            if page.len() > MAX_PAGE {
                return Err(CreditsError::Invalid);
            }
        }
        Ok(years_from_page(&String::from_utf8_lossy(&page)))
    }
}

fn endpoint(segments: &[&str]) -> Result<Url, CreditsError> {
    let mut url = Url::parse(API).map_err(|_| CreditsError::Invalid)?;
    url.path_segments_mut()
        .map_err(|_| CreditsError::Invalid)?
        .extend(segments);
    Ok(url)
}

fn valid_slug(slug: &str) -> Result<&str, CreditsError> {
    let ok = (1..=80).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(slug)
    } else {
        Err(CreditsError::Invalid)
    }
}

/// Lowercase with the common Latin accents folded, so "Lluís" and "Lluis" meet.
fn fold(text: &str) -> String {
    text.trim()
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            '’' => '\'',
            c => c,
        })
        .collect()
}

/// The first search result that is the queried film, preferring one whose title is the
/// queried title.
fn search_from_json(results: &Value, film: &FilmQuery<'_>) -> Option<u64> {
    let names: Vec<String> = [Some(film.title), film.original_title]
        .into_iter()
        .flatten()
        .map(fold)
        .collect();
    let wanted: Vec<String> = film.directors.iter().map(|name| fold(name)).collect();
    let mut matches = results["films"]
        .as_array()?
        .iter()
        .take(MAX_SEARCH)
        .filter_map(|candidate| {
            let id = candidate["id"].as_u64()?;
            let titles: Vec<String> = ["title", "original_title"]
                .iter()
                .filter_map(|field| text(&candidate[*field]))
                .map(|title| fold(&title))
                .collect();
            let named = titles.iter().any(|title| names.contains(title));
            let year_ok = match film.year {
                Some(year) => candidate["year"]
                    .as_i64()
                    .is_some_and(|found| (found - i64::from(year)).abs() <= 1),
                None => named,
            };
            let directors: Vec<String> = candidate["directors"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|director| text(&director["name"]))
                        .map(|name| fold(&name))
                        .collect()
                })
                .unwrap_or_default();
            // A shared director confirms a retitled film; a different one rules a namesake
            // out. Without either, only the title can say it is the same film.
            let known = !wanted.is_empty() && !directors.is_empty();
            let shared = directors.iter().any(|d| wanted.contains(d));
            let same = if known { shared } else { named };
            (year_ok && same).then_some((id, named))
        });
    let first = matches.next()?;
    if first.1 {
        return Some(first.0);
    }
    Some(matches.find(|(_, named)| *named).unwrap_or(first).0)
}

fn result_from_status(status: &str) -> Option<AwardResult> {
    Some(match status {
        "won" => AwardResult::Won,
        "second_place" | "third_place" => AwardResult::RunnerUp,
        "special_mention" | "honorable_mention" => AwardResult::Mention,
        "nominated" => AwardResult::Nominated,
        "screening" => AwardResult::Selected,
        _ => return None,
    })
}

/// An image URL we are willing to hand to the image loader: https, MUBI's CDN, bounded.
fn image(value: &Value) -> Option<Url> {
    let raw = value.as_str()?;
    if raw.len() > MAX_URL {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    (url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| IMAGE_HOSTS.contains(&host)))
    .then_some(url)
}

fn event_from_json(event: &Value) -> Option<Event> {
    let slug = text(&event["slug"])?;
    valid_slug(&slug).ok()?;
    Some(Event {
        id: event["id"].as_u64()?,
        slug,
        logo: image(&event["white_logo_url"]),
        cover: image(&event["cover_url"]),
    })
}

fn awards_from_json(entries: &Value) -> Vec<Award> {
    let Some(entries) = entries.as_array() else {
        return vec![];
    };
    entries
        .iter()
        .filter_map(|entry| {
            let result = result_from_status(entry["status"].as_str()?)?;
            let body = text(&entry["industry_event"]["name"])?;
            Some(Award {
                body,
                category: text(&entry["display_text"]),
                year: entry["year"]
                    .as_i64()
                    .and_then(|year| i32::try_from(year).ok()),
                result,
                event: event_from_json(&entry["industry_event"]),
            })
        })
        .take(MAX_AWARDS)
        .collect()
}

/// The festival from `/industry_events/{slug}`; the years are filled in from its web page.
fn festival_from_json(event: &Value) -> Option<Festival> {
    Some(Festival {
        name: text(&event["name"])?,
        event: event_from_json(event)?,
        years: vec![],
    })
}

fn films_from_json(page: &Value) -> FestivalPage {
    let empty = vec![];
    let films = page["films"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .take(MAX_FILMS)
        .filter_map(|film| {
            Some(FestivalFilm {
                id: film["id"].as_u64(),
                title: text(&film["title"])?,
                year: film["year"]
                    .as_i64()
                    .and_then(|year| i32::try_from(year).ok()),
                directors: film["directors"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .filter_map(|director| text(&director["name"]))
                    .take(8)
                    .collect(),
                still: image(&film["stills"]["standard"]),
                synopsis: film["short_synopsis"]
                    .as_str()
                    .map(str::trim)
                    .filter(|synopsis| !synopsis.is_empty())
                    .map(|synopsis| synopsis.chars().take(MAX_SYNOPSIS).collect()),
                minutes: film["duration"]
                    .as_u64()
                    .and_then(|minutes| u32::try_from(minutes).ok())
                    .filter(|minutes| *minutes > 0),
                genres: film["genres"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .filter_map(text)
                    .take(4)
                    .collect(),
                result: None,
            })
        })
        .collect();
    let next_page = page["meta"]["next_page"]
        .as_u64()
        .and_then(|next| u32::try_from(next).ok());
    FestivalPage { films, next_page }
}

/// `/industry_events/{id}/film_entries`: each film's entries at the edition, reduced to the
/// best one. Entries for another event are skipped, should MUBI ever mix them in.
fn results_from_json(films: &Value, event_id: u64) -> HashMap<u64, (AwardResult, Option<String>)> {
    let empty = vec![];
    let mut results = HashMap::new();
    for film in films.as_array().unwrap_or(&empty).iter().take(MAX_ENTRIES) {
        let Some(id) = film["film_id"].as_u64() else {
            continue;
        };
        let best = film["industry_event_entries"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter(|entry| {
                entry["industry_event"]["id"]
                    .as_u64()
                    .is_none_or(|event| event == event_id)
            })
            .filter_map(|entry| {
                let result = entry["status"].as_str().and_then(result_from_status)?;
                Some((result, text(&entry["display_text"])))
            })
            .min_by_key(|(result, _)| *result);
        if let Some(best) = best {
            results.insert(id, best);
        }
    }
    results
}

/// `yearsAvailable` from the festival page's `__NEXT_DATA__` JSON, newest first.
fn years_from_page(html: &str) -> Vec<i32> {
    const KEY: &str = "\"yearsAvailable\":[";
    let Some(data) = html.find("id=\"__NEXT_DATA__\"") else {
        return vec![];
    };
    let rest = &html[data..];
    let Some(start) = rest.find(KEY) else {
        return vec![];
    };
    let list = &rest[start + KEY.len() - 1..];
    let Some(end) = list.find(']') else {
        return vec![];
    };
    let Ok(Value::Array(years)) = serde_json::from_str::<Value>(&list[..=end]) else {
        return vec![];
    };
    let mut years: Vec<i32> = years
        .iter()
        .filter_map(Value::as_i64)
        .filter(|year| (1800..=2200).contains(year))
        .filter_map(|year| i32::try_from(year).ok())
        .take(MAX_YEARS)
        .collect();
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();
    years
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn aftersun<'a>(year: Option<i32>, directors: &'a [String]) -> FilmQuery<'a> {
        FilmQuery {
            title: "Aftersun",
            original_title: None,
            year,
            directors,
        }
    }

    #[test]
    fn search_picks_the_film_by_year_and_director() {
        let results = fixture(include_str!("mubi_fixtures/search.json"));
        let wells = ["Charlotte Wells".to_owned()];
        assert_eq!(
            search_from_json(&results, &aftersun(Some(2022), &wells)),
            Some(340287)
        );
        let galter = ["lluis GALTER".to_owned()];
        assert_eq!(
            search_from_json(&results, &aftersun(Some(2021), &galter)),
            Some(344731)
        );
        let wrong = ["Someone Else".to_owned()];
        assert_eq!(
            search_from_json(&results, &aftersun(Some(2022), &wrong)),
            None
        );
        assert_eq!(
            search_from_json(&results, &aftersun(Some(1999), &wells)),
            None
        );
    }

    #[test]
    fn search_without_a_year_needs_the_exact_title() {
        let results = fixture(include_str!("mubi_fixtures/search.json"));
        let wells = ["Charlotte Wells".to_owned()];
        assert_eq!(
            search_from_json(&results, &aftersun(None, &wells)),
            Some(340287)
        );
        let other = FilmQuery {
            title: "After Sun",
            ..aftersun(None, &wells)
        };
        assert_eq!(search_from_json(&results, &other), None);
        let no_directors = aftersun(Some(2006), &[]);
        assert_eq!(search_from_json(&results, &no_directors), Some(444926));
    }

    #[test]
    fn exact_title_beats_an_earlier_near_match() {
        let results = json!({"films": [
            {"id": 1, "title": "Aftersun 2", "year": 2022},
            {"id": 2, "title": "AFTERSUN", "year": 2023},
        ]});
        assert_eq!(
            search_from_json(&results, &aftersun(Some(2022), &[])),
            Some(2)
        );
    }

    #[test]
    fn a_namesake_needs_the_title_or_a_shared_director() {
        let results = json!({"films": [
            {"id": 1, "title": "Something Else", "year": 2022},
            {"id": 2, "title": "Les Années", "year": 2022, "directors": [{"name": "Charlotte Wells"}]},
        ]});
        // Same year, no directors to compare, different title: not the same film.
        assert_eq!(search_from_json(&results, &aftersun(Some(2022), &[])), None);
        // A retitled film is still found by its director.
        let wells = ["Charlotte Wells".to_owned()];
        assert_eq!(
            search_from_json(&results, &aftersun(Some(2022), &wells)),
            Some(2)
        );
    }

    #[test]
    fn statuses_map_and_unknown_ones_are_skipped() {
        let awards = awards_from_json(&fixture(include_str!("mubi_fixtures/entries.json")));
        let results: Vec<_> = awards.iter().map(|award| award.result).collect();
        assert_eq!(
            results,
            [
                AwardResult::Nominated,
                AwardResult::Won,
                AwardResult::Selected,
                AwardResult::Won
            ]
        );
        assert_eq!(awards[0].body, "Cannes Film Festival");
        assert_eq!(
            awards[0].category.as_deref(),
            Some("Critics Week Grand Prize")
        );
        assert_eq!(awards[0].year, Some(2022));
        assert_eq!(awards[2].category, None);
        let event = awards[0].event.as_ref().unwrap();
        assert_eq!((event.id, event.slug.as_str()), (1, "cannes"));
        assert!(event.logo.is_some() && event.cover.is_some());
        let all = [
            ("won", AwardResult::Won),
            ("second_place", AwardResult::RunnerUp),
            ("special_mention", AwardResult::Mention),
            ("honorable_mention", AwardResult::Mention),
            ("nominated", AwardResult::Nominated),
            ("screening", AwardResult::Selected),
        ];
        for (status, result) in all {
            assert_eq!(result_from_status(status), Some(result));
        }
    }

    #[test]
    fn images_off_the_cdn_are_dropped() {
        let awards = awards_from_json(&fixture(include_str!("mubi_fixtures/entries.json")));
        let event = awards[3].event.as_ref().unwrap();
        assert_eq!((event.logo.clone(), event.cover.clone()), (None, None));
        assert!(image(&json!("https://assets.mubicdn.net/a.jpg")).is_some());
        assert!(image(&json!("https://images.mubicdn.net.evil.test/a.jpg")).is_none());
        assert!(image(&json!("http://images.mubicdn.net/a.jpg")).is_none());
        let long = format!("https://images.mubicdn.net/{}", "a".repeat(2048));
        assert!(image(&json!(long)).is_none());
    }

    #[test]
    fn entries_without_an_event_name_are_skipped() {
        let entries = json!([
            {"status": "won", "year": 2020, "industry_event": {"id": 1, "slug": "x"}},
            {"status": "won", "year": 2020},
        ]);
        assert!(awards_from_json(&entries).is_empty());
    }

    #[test]
    fn slugs_are_validated_before_they_reach_a_url() {
        for bad in ["../x", "a b", "", "Cannes", "a/b", &"a".repeat(81)] {
            assert_eq!(valid_slug(bad), Err(CreditsError::Invalid), "{bad}");
        }
        assert_eq!(valid_slug("berlin-2"), Ok("berlin-2"));
        assert_eq!(
            endpoint(&["industry_events", "a%b"]).unwrap().path(),
            "/v4/industry_events/a%25b"
        );
    }

    #[test]
    fn festival_comes_from_the_event_object() {
        let festival =
            festival_from_json(&fixture(include_str!("mubi_fixtures/event.json"))).unwrap();
        assert_eq!(festival.name, "Tribeca Film Festival");
        assert_eq!(
            (festival.event.id, festival.event.slug.as_str()),
            (35, "tribeca")
        );
        assert!(festival.event.logo.is_some() && festival.years.is_empty());
        assert!(festival_from_json(&json!({"name": "X"})).is_none());
    }

    #[test]
    fn festival_films_parse_results_and_the_next_page() {
        let page = films_from_json(&fixture(include_str!("mubi_fixtures/films.json")));
        assert_eq!(page.next_page, Some(2));
        assert_eq!(page.films.len(), 3);
        assert_eq!(page.films[0].title, "Aftersun");
        assert_eq!(page.films[0].directors, ["Charlotte Wells"]);
        assert!(page.films[0].still.is_some());
        // The list's highlighted award is the film's best anywhere, so it is not used.
        assert!(page.films.iter().all(|film| film.result.is_none()));
        let last = films_from_json(&json!({"films": [], "meta": {"next_page": null}}));
        assert_eq!(last.next_page, None);
    }

    #[test]
    fn festival_results_keep_each_films_best_entry_at_the_edition() {
        let entry = |status: &str, text: &str, event: u64| json!({"status": status, "display_text": text, "industry_event": {"id": event}});
        let films = json!([
            // Decision to Leave at IndieWire 2022: a nomination and a win; Cannes is not here.
            {"film_id": 281688, "industry_event_entries": [
                entry("nominated", "Best Director", 14),
                entry("won", "Best Cinematography", 14),
                entry("won", "Best Director", 1),
            ]},
            {"film_id": 339480, "industry_event_entries": [entry("nominated", "Best Film", 14)]},
            {"film_id": 1, "industry_event_entries": [entry("unknown", "x", 14)]},
            {"industry_event_entries": [entry("won", "No id", 14)]},
        ]);
        let results = results_from_json(&films, 14);
        assert_eq!(
            results[&281688],
            (AwardResult::Won, Some("Best Cinematography".to_owned()))
        );
        assert_eq!(results[&339480].0, AwardResult::Nominated);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn years_come_from_the_next_data_script_newest_first() {
        let html = include_str!("mubi_fixtures/festival_page.html");
        assert_eq!(years_from_page(html), [2026, 2025, 2024, 2002]);
        assert!(years_from_page("<html>\"yearsAvailable\":[2020]</html>").is_empty());
        assert!(years_from_page("<script id=\"__NEXT_DATA__\">{}</script>").is_empty());
    }

    #[tokio::test]
    #[ignore = "talks to api.mubi.com"]
    async fn live_indiewire_2022_results() {
        let results = MubiClient::new()
            .unwrap()
            .festival_results(14, 2022)
            .await
            .unwrap();
        // Decision to Leave won at Cannes, not IndieWire's Best Director, which it was up for.
        assert_eq!(
            results[&281688],
            (AwardResult::Won, Some("Best Cinematography".to_owned()))
        );
    }

    #[tokio::test]
    #[ignore = "talks to api.mubi.com"]
    async fn live_aftersun() {
        let client = MubiClient::new().unwrap();
        let directors = ["Charlotte Wells".to_owned()];
        let id = client
            .find_film(&aftersun(Some(2022), &directors))
            .await
            .unwrap()
            .expect("MUBI knows Aftersun");
        assert_eq!(id, 340287);
        tokio::time::sleep(Duration::from_secs(2)).await;
        let awards = client.awards(id).await.unwrap();
        assert!(awards.len() > 100, "{} awards", awards.len());
        assert!(
            awards
                .iter()
                .any(|award| award.event.as_ref().is_some_and(|e| e.logo.is_some()))
        );
    }
}
