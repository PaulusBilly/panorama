//! Play straight from Home: the player opens at once, and its loader covers the source and
//! subtitle lookups the film page used to run before handing over.
use super::selection::{
    LastSource, PlaybackSelection, SourceRef, StartMode, SubtitleLookup, binge_source, first_source,
};
use crate::{
    app_state::AppState,
    services::{Job, Services},
};
use futures::{FutureExt, StreamExt, channel::mpsc};
use gpui::{Context, Task};
use panorama_core::{
    addons::{AddonKey, FilmDetails, Kind, StreamGroup, StreamSource, StreamState, StreamsEvent},
    party::PartyRelease,
    stremio::{Descriptor, ResumePoint},
    torrent::TorrentSource,
};
use std::sync::Arc;

/// Which source a launch starts with.
#[derive(Clone, Debug, Default)]
pub enum Pick {
    /// The source this video last played from, else the best ranked.
    #[default]
    Ranked,
    /// The best stream in this binge group, so an episode continues the last one's release.
    Binge(String),
    /// A watch party's release, found among this install's own streams.
    Party(PartyRelease),
}

impl Pick {
    /// The source to start with while addons are still answering; `None` holds out for more.
    pub fn early(&self, groups: &[StreamGroup], last: Option<&LastSource>) -> Option<SourceRef> {
        match (self, last) {
            (Self::Binge(group), _) => binge_source(groups, group),
            (Self::Party(release), _) => panorama_core::party::find(release, groups)
                .filter(|&(.., exact)| exact)
                .map(|(group, stream, _)| SourceRef { group, stream }),
            (Self::Ranked, Some(last)) => last.find(groups),
            (Self::Ranked, None) => first_source(groups),
        }
    }

    /// The source to start with once every addon has answered.
    pub fn last(&self, groups: &[StreamGroup], last: Option<&LastSource>) -> Option<SourceRef> {
        let found = match self {
            Self::Party(release) => panorama_core::party::find(release, groups)
                .map(|(group, stream, _)| SourceRef { group, stream }),
            Self::Ranked | Self::Binge(_) => last.and_then(|last| last.release(groups)),
        };
        found.or_else(|| first_source(groups))
    }
}

impl Pick {
    /// Like [`Pick::last`], for when no more addons will answer. A party release none of this
    /// install's addons offer, but whose host plays a torrent, plays that torrent from a group of
    /// its own appended to `groups`.
    pub fn settle(
        &self,
        groups: &mut Vec<StreamGroup>,
        last: Option<&LastSource>,
    ) -> Option<SourceRef> {
        if let Self::Party(release) = self
            && panorama_core::party::find(release, groups).is_none()
            && let Some(group) = host_copy(release)
        {
            groups.push(group);
            return Some(SourceRef {
                group: groups.len() - 1,
                stream: 0,
            });
        }
        self.last(groups, last)
    }
}

/// The host's torrent as a source of its own, when the release names one.
fn host_copy(release: &PartyRelease) -> Option<StreamGroup> {
    let torrent = release.torrent.as_ref()?;
    TorrentSource::from_stream(&torrent.info_hash, torrent.file_idx.map(u32::from), &[]).ok()?;
    let stream = serde_json::json!({
        "infoHash": torrent.info_hash,
        "fileIdx": torrent.file_idx,
    });
    Some(StreamGroup {
        addon: serde_json::from_value::<AddonKey>("party.host#copy".into()).ok()?,
        name: "Host's copy".into(),
        state: StreamState::Ready(vec![StreamSource {
            stream: serde_json::from_value(stream).ok()?,
            name: Some("Host's copy".into()),
            title: None,
            description: None,
        }]),
    })
}

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
        pick: Pick,
        cx: &mut Context<Self>,
    ) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let id = film.id.clone();
        let kind = film.kind;
        let kept = film.clone();
        let addons = self.installed.clone();
        // The source this video last played from, tried before the ranking's pick.
        let last = services.last_source(&video_id);
        let subtitles = subtitle_lookup(&services, addons.clone(), film.kind, video_id.clone());
        // Which metadata addon the progress belongs to; ready long before the streams.
        let meta_base = Job(services.runtime.clone().spawn({
            let (services, id) = (services.clone(), id.clone());
            async move { services.metadata_base(kind, &id).await }
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
                let mut latest = vec![];
                while let Some(event) = receiver.next().await {
                    let StreamsEvent::Groups(groups) = event else {
                        continue;
                    };
                    let picked = state.update(cx, |state, cx| {
                        state.streams = Some((id.clone(), Arc::new(groups.clone())));
                        // Holding out for the release in progress, or for the source this video
                        // last played from; without either, the best so far starts at once.
                        let source = pick.early(&groups, last.as_ref());
                        latest = groups.clone();
                        if let Some(source) = source
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
                    let answered = latest.len();
                    let fallback = pick.settle(&mut latest, last.as_ref());
                    if latest.len() != answered {
                        state.streams = Some((id.clone(), Arc::new(latest.clone())));
                    }
                    if let Some(source) = fallback
                        && let Some(film) = film.take()
                    {
                        state.playback = Some(PlaybackSelection {
                            film,
                            video_id,
                            groups: latest,
                            source,
                            start,
                            meta_base,
                            subtitles: vec![],
                            late_subtitles: Some(subtitles),
                        });
                        cx.notify();
                        return;
                    }
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
