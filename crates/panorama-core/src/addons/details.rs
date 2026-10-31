use futures::{FutureExt, SinkExt, StreamExt, future::join_all, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use stremio_core::types::addon::{ResourcePath, ResourceResponse};

use super::{
    AddonClient, AddonKey, DetailsStream, FailureKind, FilmDetails, Kind, ResourceEvent,
    StreamGroup, StreamState, StreamsEvent, StreamsStream, SubtitleSource, delivery, sanitize,
    supports,
};
use crate::{store::Key, stremio::Descriptor};

#[derive(Serialize, Deserialize)]
struct CachedMeta {
    addon: AddonKey,
    details: FilmDetails,
}

impl AddonClient {
    /// The transport URL of the addon that supplied the cached metadata, for the core's
    /// playback target. It can carry addon credentials, so it must never be logged.
    pub async fn metadata_base(
        &self,
        addons: &[Descriptor],
        kind: Kind,
        film_id: &str,
    ) -> Option<url::Url> {
        if !sanitize::valid_id(film_id) {
            return None;
        }
        let cached = self
            .read::<CachedMeta>(Key::typed_meta(kind.stremio(), film_id).ok()?)
            .await
            .ok()??;
        let cinemeta = self.cinemeta();
        self.installations(addons)
            .chain(film_id.starts_with("tt").then_some(cinemeta))
            .find(|addon| addon.key == cached.addon)
            .map(|addon| addon.descriptor.transport_url.clone())
    }

    /// Delivers cached metadata then tries the explicit or persisted preferred addon,
    /// matching installed meta resources, and Cinemeta for IMDb IDs, with 8s deadlines.
    /// `kind` is the title's own type; the returned details carry its episodes.
    pub fn details(
        &self,
        addons: &[Descriptor],
        kind: Kind,
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
                let Ok(key) = Key::typed_meta(kind.stremio(), &id) else {
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
                let path = ResourcePath::without_extra("meta", kind.stremio(), &id);
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
                                            if details.kind != Kind::Movie {
                                                details.episodes = sanitize::episodes(&meta.videos);
                                            }
                                            if !response.credits.director.is_empty() {
                                                details.director = response.credits.director;
                                            }
                                            if !response.credits.cast.is_empty() {
                                                details.cast = response.credits.cast;
                                            }
                                            details.people = response.credits.people;
                                            if let Some(credits) = response
                                                .credits
                                                .previews
                                                .iter()
                                                .find(|p| p.id == id)
                                            {
                                                if credits.country.is_some() {
                                                    details
                                                        .origin_country
                                                        .clone_from(&credits.country);
                                                }
                                                details
                                                    .original_name
                                                    .clone_from(&credits.original_name);
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

    /// Subtitle files from every addon offering them for the video, queried concurrently with
    /// 10s deadlines. An addon that fails adds nothing: subtitles never hold up playback.
    /// `video_id` is the film's ID, or an episode's `Video.id`, passed on untouched.
    pub async fn subtitles(
        &self,
        addons: &[Descriptor],
        kind: Kind,
        video_id: &str,
    ) -> Vec<SubtitleSource> {
        if !sanitize::valid_id(video_id) {
            return vec![];
        }
        let path = ResourcePath::without_extra("subtitles", kind.stremio(), video_id);
        let plan = self
            .installations(addons)
            .filter(|addon| supports(addon, &path, false))
            .collect::<Vec<_>>();
        let requests = plan.iter().map(|addon| {
            let path = path.clone();
            async move {
                self.request(addon, path, 10)
                    .await
                    .and_then(|response| {
                        sanitize::subtitles(response, &addon.descriptor.manifest.name)
                    })
                    .unwrap_or_default()
            }
        });
        join_all(requests).await.into_iter().flatten().collect()
    }

    /// Queries matching stream resources concurrently, with isolated 15s deadlines.
    /// Links are never cached; an unrecognized film explicitly yields `NoSources`.
    /// `video_id` is the film's ID, or an episode's `Video.id`, passed on untouched.
    pub fn streams(&self, addons: &[Descriptor], kind: Kind, video_id: &str) -> StreamsStream {
        let client = self.clone();
        let id = video_id.to_owned();
        let installed = self.installations(addons).collect::<Vec<_>>();
        delivery(&self.runtime, move |mut sender| {
            async move {
                if !sanitize::valid_id(&id) {
                    let _ = sender
                        .send(StreamsEvent::Failed(FailureKind::InvalidInput))
                        .await;
                    return;
                }
                let path = ResourcePath::without_extra("stream", kind.stremio(), &id);
                let plan = installed
                    .into_iter()
                    .filter(|addon| supports(addon, &path, false))
                    .collect::<Vec<_>>();
                if plan.is_empty() {
                    let _ = sender.send(StreamsEvent::NoSources).await;
                    return;
                }
                let mut answered = vec![None; plan.len()];
                let requests = plan.into_iter().enumerate().map(|(index, addon)| {
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
                        let group = StreamGroup {
                            addon: addon.key,
                            name: sanitize::text(&addon.descriptor.manifest.name, 300),
                            state,
                        };
                        (index, group)
                    }
                });
                let mut pending = requests.collect::<FuturesUnordered<_>>();
                while let Some((index, group)) = pending.next().await {
                    let playable = matches!(group.state, StreamState::Ready(_));
                    answered[index] = Some(group);
                    if playable && !pending.is_empty() {
                        let partial = answered.iter().flatten().cloned().collect();
                        if sender.send(StreamsEvent::Groups(partial)).await.is_err() {
                            return;
                        }
                    }
                }
                let groups = answered.into_iter().flatten().collect::<Vec<_>>();
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
