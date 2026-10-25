//! Play straight from Home: the player opens at once, and its loader covers the source and
//! subtitle lookups the film page used to run before handing over.
use super::selection::{PlaybackSelection, StartMode, SubtitleLookup, first_source};
use crate::{
    app_state::AppState,
    services::{Job, Services},
};
use futures::{FutureExt, StreamExt, channel::mpsc};
use gpui::{Context, Task};
use panorama_core::{
    addons::{FilmDetails, StreamsEvent},
    stremio::{Descriptor, ResumePoint},
};
use std::sync::Arc;

/// A play started outside the film page, until the player closes.
pub struct Launch {
    /// The film's id, matching the player route.
    pub id: String,
    /// The film's name, for the player while it waits.
    pub name: String,
    /// Every addon answered and none had a playable source.
    pub failed: bool,
    _task: Task<()>,
}

/// Ask the addons for a film's subtitle files. The lookup owns its job, so it keeps running
/// for the player after whoever started it has gone.
pub fn subtitle_lookup(
    services: &Arc<Services>,
    addons: Vec<Descriptor>,
    id: String,
) -> SubtitleLookup {
    let lookup = services.clone();
    let mut job = Job(services.runtime.clone().spawn(async move {
        match &lookup.addons {
            Some(client) => client.subtitles(&addons, &id).await,
            None => vec![],
        }
    }));
    async move { (&mut job.0).await.unwrap_or_default() }
        .boxed()
        .shared()
}

impl AppState {
    /// Look up the film's sources and subtitles for a player that is already open. The first
    /// playable source becomes the selection the player starts with; later addon results
    /// still reach it through `streams`.
    pub fn launch(
        &mut self,
        film: FilmDetails,
        resume: Option<ResumePoint>,
        cx: &mut Context<Self>,
    ) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let id = film.id.clone();
        let name = film.name.clone();
        let addons = self.installed.clone();
        let subtitles = subtitle_lookup(&services, addons.clone(), id.clone());
        // Which metadata addon the progress belongs to; ready long before the streams.
        let meta_base = Job(services.runtime.clone().spawn({
            let (services, id) = (services.clone(), id.clone());
            async move { services.metadata_base(&id).await }
        }));
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.clone().spawn({
            let id = id.clone();
            async move {
                match &services.addons {
                    Some(client) => {
                        let mut events = client.streams(&addons, &id);
                        while let Some(event) = events.next().await {
                            if sender.unbounded_send(event).is_err() {
                                break;
                            }
                        }
                    }
                    None => {
                        let _ = sender.unbounded_send(crate::fixtures::streams(false));
                    }
                }
            }
        }));
        let start = resume.map_or(StartMode::Restart, |point| {
            StartMode::Resume(point.offset_secs)
        });
        let task = cx.spawn({
            let id = id.clone();
            async move |state, cx| {
                let _job = job;
                let mut meta_base = meta_base;
                let meta_base = (&mut meta_base.0).await.ok().flatten();
                let mut film = Some(film);
                while let Some(event) = receiver.next().await {
                    let StreamsEvent::Groups(groups) = event else {
                        continue;
                    };
                    let picked = state.update(cx, |state, cx| {
                        state.streams = Some((id.clone(), Arc::new(groups.clone())));
                        if let Some(source) = first_source(&groups)
                            && let Some(film) = film.take()
                        {
                            state.playback = Some(PlaybackSelection {
                                film,
                                groups,
                                source,
                                start,
                                meta_base: meta_base.clone(),
                                subtitles: vec![],
                                late_subtitles: Some(subtitles.clone()),
                            });
                        }
                        cx.notify();
                    });
                    if picked.is_err() {
                        return;
                    }
                }
                let _ = state.update(cx, |state, cx| {
                    if film.is_some()
                        && let Some(launch) = state.launch.as_mut().filter(|l| l.id == id)
                    {
                        launch.failed = true;
                        cx.notify();
                    }
                });
            }
        });
        self.playback = None;
        self.launch = Some(Launch {
            id,
            name,
            failed: false,
            _task: task,
        });
        cx.notify();
    }
}
