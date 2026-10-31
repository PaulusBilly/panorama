//! Play straight from Home: the player opens at once, and its loader covers the source and
//! subtitle lookups the film page used to run before handing over.
use super::selection::{PlaybackSelection, StartMode, SubtitleLookup, binge_source, first_source};
use crate::{
    app_state::AppState,
    services::{Job, Services},
};
use futures::{FutureExt, StreamExt, channel::mpsc};
use gpui::{Context, Task};
use panorama_core::{
    addons::{FilmDetails, Kind, StreamsEvent},
    stremio::{Descriptor, ResumePoint},
};
use std::sync::Arc;

/// A play started outside the film page, until the player closes.
pub struct Launch {
    /// The film's id, matching the player route.
    pub id: String,
    /// What is playing: the film's id, or an episode's.
    pub video_id: String,
    /// The film and resume point, for the player while it waits and to look again.
    pub film: FilmDetails,
    pub resume: Option<ResumePoint>,
    /// Every addon answered and none had a playable source.
    pub failed: bool,
    _task: Task<()>,
}

/// Ask the addons for a film's subtitle files. The lookup owns its job, so it keeps running
/// for the player after whoever started it has gone.
pub fn subtitle_lookup(
    services: &Arc<Services>,
    addons: Vec<Descriptor>,
    kind: Kind,
    id: String,
) -> SubtitleLookup {
    let lookup = services.clone();
    let mut job = Job(services.runtime.clone().spawn(async move {
        match &lookup.addons {
            Some(client) => client.subtitles(&addons, kind, &id).await,
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
        video_id: String,
        resume: Option<ResumePoint>,
        binge_group: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let id = film.id.clone();
        let kind = film.kind;
        let kept = film.clone();
        let addons = self.installed.clone();
        let subtitles = subtitle_lookup(&services, addons.clone(), film.kind, video_id.clone());
        // Which metadata addon the progress belongs to; ready long before the streams.
        let meta_base = Job(services.runtime.clone().spawn({
            let (services, id) = (services.clone(), id.clone());
            async move { services.metadata_base(&id).await }
        }));
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.clone().spawn({
            let video_id = video_id.clone();
            async move {
                match &services.addons {
                    Some(client) => {
                        let mut events = client.streams(&addons, kind, &video_id);
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
        let start = resume.as_ref().map_or(StartMode::Restart, |point| {
            StartMode::Resume(point.offset_secs)
        });
        let task = cx.spawn({
            let id = id.clone();
            let video_id = video_id.clone();
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
                        let keeping = binge_group.as_deref();
                        let matching = keeping.and_then(|group| binge_source(&groups, group));
                        if let Some(source) = matching.or_else(|| first_source(&groups))
                            && let Some(film) = film.take()
                        {
                            state.playback = Some(PlaybackSelection {
                                film,
                                video_id: video_id.clone(),
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
            video_id,
            film: kept,
            resume,
            failed: false,
            _task: task,
        });
        cx.notify();
    }
}
