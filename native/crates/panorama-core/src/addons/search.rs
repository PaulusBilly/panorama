use std::collections::HashSet;

use futures::{FutureExt, SinkExt, StreamExt, stream};
use stremio_core::types::addon::{ExtraValue, ResourcePath};

use super::{AddonClient, FailureKind, ResourceEvent, SearchStream, delivery, sanitize};
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
                let thin = items
                    .iter()
                    .take(20)
                    .enumerate()
                    .filter(|(_, film)| {
                        film.poster.is_none()
                            || film.name.trim().is_empty()
                            || film.release_info.is_none()
                    })
                    .map(|(index, film)| {
                        (
                            index,
                            film.id.clone(),
                            film.name.trim().is_empty(),
                            film.poster.is_none(),
                            film.release_info.is_none(),
                        )
                    })
                    .collect::<Vec<_>>();
                let fill = thin.into_iter().map(|(index, id, name, poster, release)| {
                    client
                        .details(&addons, &id, None)
                        .map(move |event| (index, name, poster, release, event))
                });
                let mut fill = stream::iter(fill).flatten_unordered(4);
                while let Some((index, name, poster, release, event)) = fill.next().await {
                    let details = match event {
                        ResourceEvent::Fresh(details) | ResourceEvent::CacheHit(details) => details,
                        ResourceEvent::Failed(FailureKind::Offline) => {
                            failure = Some(FailureKind::Offline);
                            continue;
                        }
                        _ => continue,
                    };
                    let film = &mut items[index];
                    if name {
                        film.name = details.name;
                    }
                    if poster {
                        film.poster = details.poster;
                    }
                    if release {
                        film.release_info = details.release_info;
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
