use futures::{FutureExt, SinkExt, future::join_all};
use serde::{Deserialize, Serialize};
use stremio_core::types::addon::{ResourcePath, ResourceResponse};

use super::{
    AddonClient, AddonKey, DetailsStream, FailureKind, FilmDetails, ResourceEvent, StreamGroup,
    StreamState, StreamsEvent, StreamsStream, delivery, sanitize, supports,
};
use crate::{store::Key, stremio::Descriptor};

#[derive(Serialize, Deserialize)]
struct CachedMeta {
    addon: AddonKey,
    details: FilmDetails,
}

impl AddonClient {
    /// Delivers cached metadata then tries the explicit or persisted preferred addon,
    /// matching installed meta resources, and Cinemeta for IMDb IDs, with 8s deadlines.
    pub fn details(
        &self,
        addons: &[Descriptor],
        film_id: &str,
        preferred: Option<AddonKey>,
    ) -> DetailsStream {
        let client = self.for_request();
        let addons = addons.to_vec();
        let id = film_id.to_owned();
        delivery(&self.runtime, move |mut sender| {
            async move {
                if !sanitize::valid_id(&id) {
                    let _ = sender
                        .send(ResourceEvent::Failed(FailureKind::InvalidInput))
                        .await;
                    return;
                }
                let Ok(key) = Key::meta(&id) else {
                    let _ = sender
                        .send(ResourceEvent::Failed(FailureKind::InvalidInput))
                        .await;
                    return;
                };
                let installed = client.installations(&addons).collect::<Vec<_>>();
                let cinemeta = client.cinemeta();
                match client.read::<CachedMeta>(key.clone()).await {
                    Ok(Some(cached))
                        if cached.details.id == id
                            && (installed.iter().any(|addon| addon.key == cached.addon)
                                || (id.starts_with("tt") && cached.addon == cinemeta.key)) =>
                    {
                        if sender
                            .send(ResourceEvent::CacheHit(cached.details))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    _ => {}
                }
                let preferred = match preferred {
                    Some(preferred) => Some(preferred),
                    None => match Key::pref("metaAddon") {
                        Ok(key) => client.read::<AddonKey>(key).await.ok().flatten(),
                        Err(_) => None,
                    },
                };
                let path = ResourcePath::without_extra("meta", "movie", &id);
                let mut plan = installed
                    .iter()
                    .filter(|addon| supports(addon, &path, false))
                    .cloned()
                    .collect::<Vec<_>>();
                if let Some(preferred) = preferred.and_then(|key| {
                    installed
                        .iter()
                        .find(|addon| addon.key == key && supports(addon, &path, true))
                }) {
                    plan.retain(|addon| addon.key != preferred.key);
                    plan.insert(0, preferred.clone());
                }
                if id.starts_with("tt") && !plan.iter().any(|addon| addon.key == cinemeta.key) {
                    plan.push(cinemeta);
                }
                let mut failure = FailureKind::InvalidResponse;
                for addon in plan {
                    let result =
                        client
                            .request(&addon, path.clone(), 8)
                            .await
                            .and_then(|response| match response.core {
                                ResourceResponse::Meta { meta } => {
                                    sanitize::film(meta.preview, false)
                                        .filter(|details| details.id == id)
                                        .map(|mut details| {
                                            if !response.credits.director.is_empty() {
                                                details.director = response.credits.director;
                                            }
                                            if !response.credits.cast.is_empty() {
                                                details.cast = response.credits.cast;
                                            }
                                            if let Some(credits) = response
                                                .credits
                                                .previews
                                                .iter()
                                                .find(|p| p.id == id)
                                                && credits.country.is_some()
                                            {
                                                details.origin_country.clone_from(&credits.country);
                                            }
                                            details
                                        })
                                        .ok_or(FailureKind::InvalidResponse)
                                }
                                _ => Err(FailureKind::InvalidResponse),
                            });
                    match result {
                        Ok(details) => {
                            let cached = CachedMeta {
                                addon: addon.key,
                                details: details.clone(),
                            };
                            let saved = client.write(key, &cached).await;
                            let _ = sender.send(ResourceEvent::Fresh(details)).await;
                            if let Err(kind) = saved {
                                let _ = sender.send(ResourceEvent::Failed(kind)).await;
                            }
                            return;
                        }
                        Err(FailureKind::Offline) => {
                            failure = FailureKind::Offline;
                        }
                        Err(kind) if failure != FailureKind::Offline => failure = kind,
                        Err(_) => {}
                    }
                }
                let _ = sender.send(ResourceEvent::Failed(failure)).await;
            }
            .boxed()
        })
    }

    /// Queries matching stream resources concurrently, with isolated 15s deadlines.
    /// Links are never cached; an unrecognized film explicitly yields `NoSources`.
    pub fn streams(&self, addons: &[Descriptor], film_id: &str) -> StreamsStream {
        let client = self.clone();
        let id = film_id.to_owned();
        let installed = self.installations(addons).collect::<Vec<_>>();
        delivery(&self.runtime, move |mut sender| {
            async move {
                if !sanitize::valid_id(&id) {
                    let _ = sender
                        .send(StreamsEvent::Failed(FailureKind::InvalidInput))
                        .await;
                    return;
                }
                let path = ResourcePath::without_extra("stream", "movie", &id);
                let plan = installed
                    .into_iter()
                    .filter(|addon| supports(addon, &path, false))
                    .collect::<Vec<_>>();
                if plan.is_empty() {
                    let _ = sender.send(StreamsEvent::NoSources).await;
                    return;
                }
                let requests = plan.into_iter().map(|addon| {
                    let client = client.clone();
                    let path = path.clone();
                    async move {
                        let state = match client
                            .request(&addon, path, 15)
                            .await
                            .and_then(sanitize::sources)
                        {
                            Ok(sources) if sources.is_empty() => StreamState::Empty,
                            Ok(sources) => StreamState::Ready(sources),
                            Err(kind) => StreamState::Failed(kind),
                        };
                        StreamGroup {
                            addon: addon.key,
                            name: sanitize::text(&addon.descriptor.manifest.name, 300),
                            state,
                        }
                    }
                });
                let groups = join_all(requests).await;
                let offline = groups
                    .iter()
                    .all(|group| group.state == StreamState::Failed(FailureKind::Offline));
                if offline {
                    let _ = sender
                        .send(StreamsEvent::Failed(FailureKind::Offline))
                        .await;
                } else {
                    let _ = sender.send(StreamsEvent::Groups(groups)).await;
                }
            }
            .boxed()
        })
    }
}
