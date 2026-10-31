//! The film page's sections below the hero. Cast and crew come with the metadata addon's
//! answer (`app_extras`, as TMDB-backed addons such as AIOMetadata send it); awards come from
//! MUBI (`crate::mubi`), else Wikidata. Both are optional: without them the section is left out.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use url::Url;

const WIKIDATA_SPARQL: &str = "https://query.wikidata.org/sparql";
/// Wikimedia asks every client to name itself.
const USER_AGENT: &str = concat!(
    "Panorama/",
    env!("CARGO_PKG_VERSION"),
    " (desktop film client)"
);
/// Award lists run to tens of kilobytes; anything far larger is not an answer.
const MAX_BODY: usize = 4 << 20;
const TIMEOUT: Duration = Duration::from_secs(10);
/// The full list "Show all" opens; billing order puts the leads first.
const MAX_PEOPLE: usize = 60;

/// Someone who made the film, with every role they had on it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Person {
    pub name: String,
    /// "Director", "Cast", "Writer", joined when one person had several.
    pub role: String,
    pub photo: Option<Url>,
}

/// One award or festival entry: a win, a nomination or a screening.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Award {
    /// The award's family or festival, e.g. "BAFTA Award" or "Cannes Film Festival".
    pub body: String,
    /// The category, e.g. "Best Newcomer"; `None` for a screening or an award with no "for" part.
    pub category: Option<String>,
    pub year: Option<i32>,
    pub result: AwardResult,
    /// The MUBI event, for its logo and festival page; `None` from Wikidata.
    pub event: Option<Event>,
}

/// How a film fared, best first: sorting ascending puts wins on top.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum AwardResult {
    Won,
    RunnerUp,
    Mention,
    Nominated,
    /// Screened at a festival, in or out of competition.
    Selected,
}

/// A MUBI award or festival.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub slug: String,
    /// White logo on transparency.
    pub logo: Option<Url>,
    pub cover: Option<Url>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreditsError {
    Network,
    Invalid,
}

/// Wikidata over HTTPS, with bounded answers.
#[derive(Clone)]
pub struct CreditsClient {
    http: reqwest::Client,
}

impl CreditsClient {
    pub fn new() -> Result<Self, CreditsError> {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .referer(false)
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| CreditsError::Network)?;
        Ok(Self { http })
    }

    /// Awards won and nominated for, newest first.
    pub async fn awards(&self, film_id: &str) -> Result<Vec<Award>, CreditsError> {
        let film = film_clause(film_id)?;
        let query = format!(
            "SELECT ?won ?awardLabel ?time WHERE {{ {film} \
             {{ ?film p:P166 ?s. ?s ps:P166 ?award. BIND(true AS ?won) }} UNION \
             {{ ?film p:P1411 ?s. ?s ps:P1411 ?award. BIND(false AS ?won) }} \
             OPTIONAL {{ ?s pq:P585 ?time }} \
             SERVICE wikibase:label {{ bd:serviceParam wikibase:language \"en,mul\". }} }} LIMIT 300"
        );
        let request = self
            .http
            .get(WIKIDATA_SPARQL)
            .query(&[("query", query.as_str())])
            .header(reqwest::header::ACCEPT, "application/sparql-results+json");
        Ok(awards_from_sparql(&json(request).await?))
    }

    /// The film's original title, when it is not an English one. Going by the title and not
    /// the film's languages keeps multilingual films such as Le Mépris.
    pub async fn original_title(&self, film_id: &str) -> Result<Option<String>, CreditsError> {
        let film = film_clause(film_id)?;
        let query = format!(
            "SELECT ?title WHERE {{ {film} ?film wdt:P1476 ?title. \
             FILTER(!LANGMATCHES(LANG(?title), \"en\")) \
             FILTER NOT EXISTS {{ ?film wdt:P1476 ?english. \
             FILTER(LANGMATCHES(LANG(?english), \"en\")) }} }} LIMIT 1"
        );
        let request = self
            .http
            .get(WIKIDATA_SPARQL)
            .query(&[("query", query.as_str())])
            .header(reqwest::header::ACCEPT, "application/sparql-results+json");
        Ok(original_from_sparql(&json(request).await?))
    }
}

/// Ids are checked here because they are written into the query.
fn film_clause(film_id: &str) -> Result<String, CreditsError> {
    if is_imdb(film_id) {
        return Ok(format!("?film wdt:P345 \"{film_id}\"."));
    }
    let id = film_id
        .strip_prefix("tmdb:")
        .and_then(|id| id.parse::<u64>().ok())
        .ok_or(CreditsError::Invalid)?;
    Ok(format!("?film wdt:P4947 \"{id}\"."))
}

fn original_from_sparql(results: &Value) -> Option<String> {
    text(&results["results"]["bindings"][0]["title"]["value"])
}

pub(crate) async fn json(request: reqwest::RequestBuilder) -> Result<Value, CreditsError> {
    let mut response = request
        .send()
        .await
        .map_err(|_| CreditsError::Network)?
        .error_for_status()
        .map_err(|_| CreditsError::Network)?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY as u64)
    {
        return Err(CreditsError::Invalid);
    }
    // Read in chunks, so a body without a length is refused before it grows past the cap.
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| CreditsError::Network)? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_BODY {
            return Err(CreditsError::Invalid);
        }
    }
    serde_json::from_slice(&bytes).map_err(|_| CreditsError::Invalid)
}

fn is_imdb(id: &str) -> bool {
    id.strip_prefix("tt").is_some_and(|digits| {
        (1..=12).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

pub(crate) fn text(value: &Value) -> Option<String> {
    let value = value.as_str()?.trim();
    (!value.is_empty()).then(|| value.chars().take(200).collect())
}

/// Directors, then the cast in billing order, then writers, from a metadata addon's
/// `app_extras`; one entry per person, with every role they had.
pub fn people_from_extras(extras: &Value) -> Vec<Person> {
    let mut people: Vec<(Option<u64>, Person)> = vec![];
    for (field, role) in [
        ("directors", "Director"),
        ("cast", "Cast"),
        ("writers", "Writer"),
    ] {
        let Some(list) = extras[field].as_array() else {
            continue;
        };
        for member in list.iter().take(MAX_PEOPLE) {
            let Some(name) = text(&member["name"]) else {
                continue;
            };
            let id = member["tmdbId"].as_u64();
            if let Some((_, person)) = people
                .iter_mut()
                .find(|(known, person)| *known == id && person.name == name)
            {
                if !person.role.split(", ").any(|known| known == role) {
                    person.role = format!("{}, {role}", person.role);
                }
                continue;
            }
            let photo = member["photo"]
                .as_str()
                .and_then(|photo| Url::parse(photo).ok())
                .filter(|url| url.scheme() == "https" && url.as_str().len() <= 2048);
            people.push((
                id,
                Person {
                    name,
                    role: role.into(),
                    photo,
                },
            ));
        }
    }
    people.truncate(MAX_PEOPLE);
    people.into_iter().map(|(_, person)| person).collect()
}

/// Wins over nominations for the same award and year, newest first.
pub fn awards_from_sparql(results: &Value) -> Vec<Award> {
    let empty = vec![];
    let rows = results["results"]["bindings"].as_array().unwrap_or(&empty);
    let mut awards: Vec<Award> = vec![];
    for row in rows {
        let Some(label) = text(&row["awardLabel"]["value"]) else {
            continue;
        };
        // An unlabelled item comes back as its id ("Q123"); it names nothing.
        if label.starts_with('Q') && label[1..].bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let result = if row["won"]["value"] == "true" {
            AwardResult::Won
        } else {
            AwardResult::Nominated
        };
        let year = row["time"]["value"]
            .as_str()
            .and_then(|time| time.get(..4))
            .and_then(|year| year.parse().ok());
        let (body, category) = match label.split_once(" for ") {
            Some((body, category)) => (body.to_owned(), Some(category.to_owned())),
            None => (label, None),
        };
        let award = Award {
            body,
            category,
            year,
            result,
            event: None,
        };
        match awards.iter_mut().find(|known| {
            known.body == award.body && known.category == award.category && known.year == award.year
        }) {
            Some(known) => known.result = known.result.min(award.result),
            None => awards.push(award),
        }
    }
    awards.sort_by(|a, b| b.year.cmp(&a.year).then(a.result.cmp(&b.result)));
    awards
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn people_list_directors_cast_then_writers_once_each() {
        let extras = json!({
            "directors": [{"name": "Charlotte Wells", "tmdbId": 9, "photo": "https://image.tmdb.org/c.jpg"}],
            "cast": [
                {"name": "Paul Mescal", "tmdbId": 1, "photo": "https://image.tmdb.org/a.jpg"},
                {"name": "Brooklyn Toulson", "tmdbId": 2, "photo": null},
                {"name": " ", "tmdbId": 3},
                {"name": "Frankie Corio", "photo": "http://insecure.test/b.jpg"},
            ],
            "writers": [{"name": "Charlotte Wells", "tmdbId": 9}],
        });
        let people = people_from_extras(&extras);
        let summary: Vec<_> = people
            .iter()
            .map(|person| format!("{}: {}", person.name, person.role))
            .collect();
        assert_eq!(
            summary,
            [
                "Charlotte Wells: Director, Writer",
                "Paul Mescal: Cast",
                "Brooklyn Toulson: Cast",
                "Frankie Corio: Cast",
            ]
        );
        assert_eq!(
            people[1].photo.as_ref().map(Url::as_str),
            Some("https://image.tmdb.org/a.jpg")
        );
        assert_eq!(people[2].photo, None);
        assert_eq!(people[3].photo, None);
        assert!(people_from_extras(&json!(null)).is_empty());
    }

    #[test]
    fn awards_merge_a_win_with_its_nomination_newest_first() {
        let row = |won: bool, label: &str, time: Option<&str>| {
            let mut row = json!({
                "won": {"value": won.to_string()},
                "awardLabel": {"value": label},
            });
            if let Some(time) = time {
                row["time"] = json!({"value": time});
            }
            row
        };
        let results = json!({"results": {"bindings": [
            row(false, "César Award for Best Foreign Film", Some("2001-01-01T00:00:00Z")),
            row(true, "César Award for Best Foreign Film", Some("2001-01-01T00:00:00Z")),
            row(true, "Cannes Film Festival Award for Best Actor", Some("2000-01-01T00:00:00Z")),
            row(false, "Q132143126", None),
            row(false, "International Submission to the Academy Awards", None),
        ]}});
        let awards = awards_from_sparql(&results);
        assert_eq!(awards.len(), 3);
        assert_eq!(
            awards[0],
            Award {
                body: "César Award".into(),
                category: Some("Best Foreign Film".into()),
                year: Some(2001),
                result: AwardResult::Won,
                event: None,
            }
        );
        assert_eq!(awards[1].body, "Cannes Film Festival Award");
        assert_eq!(awards[2].category, None);
    }

    #[test]
    fn original_title_reads_the_first_answer_and_tolerates_none() {
        let answer = json!({"results": {"bindings": [{"title": {"value": " 花樣年華 "}}]}});
        assert_eq!(original_from_sparql(&answer).as_deref(), Some("花樣年華"));
        assert_eq!(
            original_from_sparql(&json!({"results": {"bindings": []}})),
            None
        );
        assert_eq!(original_from_sparql(&json!(null)), None);
    }

    #[test]
    fn only_well_formed_ids_reach_the_queries() {
        assert!(is_imdb("tt19770238"));
        assert!(!is_imdb("tt"));
        assert!(!is_imdb("tt12\" }"));
        assert!(!is_imdb("tmdb:1"));
        assert_eq!(film_clause("tt12\" }"), Err(CreditsError::Invalid));
        assert_eq!(film_clause("tmdb:7").unwrap(), "?film wdt:P4947 \"7\".");
    }
}
