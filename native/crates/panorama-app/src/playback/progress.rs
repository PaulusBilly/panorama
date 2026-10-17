use super::{
    model::{PlaybackModel, Status},
    selection::PlaybackSelection,
};
use crate::services::Services;
use futures::future::BoxFuture;
use panorama_core::{
    discord::{DiscordPlayback, PlaybackState, discord_artwork},
    stremio::{FilmMeta, PlaybackSample, PlaybackTarget},
};
use std::sync::Arc;

pub enum Progress {
    Begin(Box<PlaybackSelection>, f64),
    Sample(String, PlaybackSample),
    Seek(String, PlaybackSample),
    Finish(String),
    Stop(String),
}

pub trait ProgressSink: Send + Sync + 'static {
    fn send(&self, event: Progress) -> BoxFuture<'_, ()>;
    fn presence(&self, selection: &PlaybackSelection, model: &PlaybackModel);
    fn clear(&self);
}

pub struct CoreProgress(pub Arc<Services>);

impl ProgressSink for CoreProgress {
    fn send(&self, event: Progress) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let Some(session) = &self.0.session else {
                return;
            };
            let session = session.lock().await;
            if !session.is_signed_in() {
                return;
            }
            match event {
                Progress::Begin(selection, seconds) => {
                    let Some(source) = selection.selected() else {
                        return;
                    };
                    let Some(client) = &self.0.addons else {
                        return;
                    };
                    let Some(group) = selection.groups.get(selection.source.group) else {
                        return;
                    };
                    let installed = session.installed_addons();
                    let Some(descriptor) = installed.iter().find(|descriptor| {
                        client
                            .addon_key(descriptor)
                            .is_ok_and(|key| key == group.addon)
                    }) else {
                        return;
                    };
                    let meta_base = selection.meta_base.clone().or_else(|| {
                        url::Url::parse("https://v3-cinemeta.strem.io/manifest.json").ok()
                    });
                    let Some(meta_base) = meta_base else {
                        return;
                    };
                    let _ = session.begin_playback(
                        PlaybackTarget {
                            film: FilmMeta {
                                id: selection.film.id.clone(),
                                name: selection.film.name.clone(),
                                poster: selection.film.poster.clone(),
                            },
                            stream: source.stream.clone(),
                            meta_base,
                            stream_base: descriptor.as_core().transport_url.clone(),
                        },
                        seconds,
                    );
                }
                Progress::Sample(id, sample) => {
                    session.report_progress(&id, sample);
                }
                Progress::Seek(id, sample) => {
                    session.report_seek(&id, sample);
                }
                Progress::Finish(id) => {
                    session.finish(&id);
                }
                Progress::Stop(id) => {
                    session.stop_playback(&id);
                }
            }
        })
    }

    fn presence(&self, selection: &PlaybackSelection, model: &PlaybackModel) {
        let Some(presence) = &self.0.discord else {
            return;
        };
        if !matches!(model.status, Status::Ready | Status::Buffering) {
            presence.clear();
            return;
        }
        let film = &selection.film;
        presence.update(DiscordPlayback {
            film_id: film.id.clone(),
            title: film.name.clone(),
            year: film.release_info.clone(),
            director_tmdb_id: None,
            director: film.director.first().cloned(),
            artwork: discord_artwork(&[
                film.background.as_ref().map(|url| url.as_str()),
                film.poster.as_ref().map(|url| url.as_str()),
            ]),
            time: model.time.unwrap_or(0.0),
            duration: model.duration.unwrap_or(0.0),
            state: if model.status == Status::Buffering {
                PlaybackState::Buffering
            } else if model.paused {
                PlaybackState::Paused
            } else {
                PlaybackState::Playing
            },
        });
    }

    fn clear(&self) {
        if let Some(presence) = &self.0.discord {
            presence.clear();
        }
    }
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
