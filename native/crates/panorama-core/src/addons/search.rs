use std::collections::HashSet;

use futures::{FutureExt, SinkExt, StreamExt, stream};
use stremio_core::types::addon::{ExtraValue, ResourcePath};

use super::{
    AddonClient, FailureKind, FilmDetails, ResourceEvent, SearchStream, delivery, sanitize,
};
use crate::{store::Key, stremio::Descriptor};

impl AddonClient {
    /// Searches up to four eligible movie catalogs concurrently, merging in account
    /// order. Enriches thin cards among the first 20 results, four at a time.
    /// Queries are trimmed and capped at 100 characters; empty queries do no I/O.
    pub fn search(&self, addons: &[Descriptor], query: &str) -> SearchStream {
        let client = self.for_request();
        let addons = addons.to_vec();
        let query = sanitize::text(query.trim(), 100);
        delivery(&self.runtime, move |mut sender| {
            async move {
                if query.is_empty() {
                    let _ = sender.send(ResourceEvent::Fresh(vec![])).await;
                    return;
                }
                let extra = [ExtraValue {
                    name: "search".into(),
                    value: query,
                }];
                let mut plan = Vec::new();
                'addons: for addon in client.installations(&addons) {
                    for catalog in addon.descriptor.manifest.catalogs.iter().take(100) {
                        if catalog.r#type == "movie"
                            && Key::catalog(addon.key.as_str(), &catalog.id).is_ok()
                            && catalog.extra.iter().any(|prop| prop.name == "search")
                            && catalog.is_extra_supported(&extra)
                        {
                            plan.push((
                                addon.clone(),
                                ResourcePath::with_extra("catalog", "movie", &catalog.id, &extra),
                            ));
                            if plan.len() == 4 {
                                break 'addons;
                            }
                        }
                    }
                }
                if addons.is_empty() {
                    plan.push((
                        client.cinemeta(),
                        ResourcePath::with_extra("catalog", "movie", "top", &extra),
                    ));
                }
                if plan.is_empty() {
                    let _ = sender.send(ResourceEvent::Fresh(vec![])).await;
                    return;
                }
                let requests = plan.into_iter().map(|(addon, path)| {
                    let client = client.clone();
                    async move {
                        client
                            .request(&addon, path, 8)
                            .await
                            .and_then(|response| sanitize::films(response, true))
                    }
                });
                let responses = stream::iter(requests).buffered(4).collect::<Vec<_>>().await;
                let mut seen = HashSet::new();
                let mut items = vec![];
                let mut failure = None;
                let mut successes = 0;
                for response in responses {
                    match response {
                        Ok(results) => {
                            successes += 1;
                            items.extend(
                                results
                                    .into_iter()
                                    .filter(|film| seen.insert(film.id.clone())),
                            );
                        }
                        Err(kind) => {
                            if failure != Some(FailureKind::Offline) {
                                failure = Some(kind);
                            }
                        }
                    }
                }
                if successes == 0 {
                    let _ = sender
                        .send(ResourceEvent::Failed(
                            failure.unwrap_or(FailureKind::InvalidResponse),
                        ))
                        .await;
                    return;
                }
                if sender
                    .send(ResourceEvent::Fresh(
                        items
                            .iter()
                            .filter(|film| !film.name.trim().is_empty())
                            .cloned()
                            .collect(),
                    ))
                    .await
                    .is_err()
                {
                    return;
                }
                let searched = items.clone();
                let thin = items
                    .iter()
                    .take(20)
                    .enumerate()
                    .filter(|(_, film)| thin(film))
                    .map(|(index, film)| (index, film.id.clone()))
                    .collect::<Vec<_>>();
                let fill = thin.into_iter().map(|(index, id)| {
                    client
                        .details(&addons, &id, None)
                        .map(move |event| (index, event))
                });
                let mut fill = stream::iter(fill).flatten_unordered(4);
                while let Some((index, event)) = fill.next().await {
                    let details = match event {
                        ResourceEvent::Fresh(details) | ResourceEvent::CacheHit(details) => details,
                        ResourceEvent::Failed(FailureKind::Offline) => {
                            failure = Some(FailureKind::Offline);
                            continue;
                        }
                        _ => continue,
                    };
                    // Fill from the card as searched, so fresh details replace cached ones.
                    let mut film = searched[index].clone();
                    fill_gaps(&mut film, details);
                    items[index] = film;
                    if sender
                        .send(ResourceEvent::Fresh(
                            items
                                .iter()
                                .filter(|film| !film.name.trim().is_empty())
                                .cloned()
                                .collect(),
                        ))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                if let Some(FailureKind::Offline) = failure {
                    let _ = sender
                        .send(ResourceEvent::Failed(FailureKind::Offline))
                        .await;
                }
            }
            .boxed()
        })
    }
}

/// Whether a search card lacks what Home's cards show: title, artwork, credits or year.
fn thin(film: &FilmDetails) -> bool {
    film.name.trim().is_empty()
        || film.poster.is_none()
        || film.background.is_none()
        || film.director.is_empty()
        || film.origin_country.is_none()
        || film.release_info.is_none()
}

/// Copy into `film` only the fields it is missing, keeping what the search result had.
/// Called on the card as searched each time, so later details replace earlier ones.
fn fill_gaps(film: &mut FilmDetails, details: FilmDetails) {
    if film.name.trim().is_empty() {
        film.name = details.name;
    }
    film.poster = film.poster.take().or(details.poster);
    film.background = film.background.take().or(details.background);
    if film.director.is_empty() {
        film.director = details.director;
    }
    film.origin_country = film.origin_country.take().or(details.origin_country);
    film.release_info = film.release_info.take().or(details.release_info);
    if film.description.is_none() {
        film.description = details.description;
    }
    if film.runtime.is_none() {
        film.runtime = details.runtime;
    }
    if film.genres.is_empty() {
        film.genres = details.genres;
    }
    if film.imdb_rating.is_none() {
        film.imdb_rating = details.imdb_rating;
    }
}

#[cfg(test)]
mod enrichment {
    use super::*;

    fn film(name: &str) -> FilmDetails {
        FilmDetails {
            id: "tt1".into(),
            name: name.into(),
            release_info: None,
            runtime: None,
            genres: Vec::new(),
            description: None,
            director: Vec::new(),
            origin_country: None,
            cast: Vec::new(),
            poster: None,
            background: None,
            logo: None,
            imdb_rating: None,
            links: Vec::new(),
        }
    }

    #[test]
    fn details_fill_only_what_the_search_card_lacks() {
        let url = |path: &str| Some(url::Url::parse(&format!("https://img.test/{path}")).unwrap());
        let mut card = FilmDetails {
            poster: url("search-poster.jpg"),
            ..film("Aftersun")
        };
        assert!(thin(&card));
        let details = FilmDetails {
            poster: url("details-poster.jpg"),
            background: url("backdrop.jpg"),
            director: vec!["Charlotte Wells".into()],
            origin_country: Some("United Kingdom".into()),
            release_info: Some("2022".into()),
            ..film("Aftersun (2022)")
        };
        fill_gaps(&mut card, details);
        assert_eq!(card.name, "Aftersun");
        assert_eq!(card.poster, url("search-poster.jpg"));
        assert_eq!(card.background, url("backdrop.jpg"));
        assert_eq!(card.director, ["Charlotte Wells"]);
        assert!(!thin(&card));
    }
}
