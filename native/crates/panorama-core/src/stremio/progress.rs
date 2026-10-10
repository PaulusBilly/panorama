//! Watch progress, resume points and the watchlist, driven through the core.
//!
//! Progress goes through the core's `Player` model exactly as the Electron
//! runtime drives it (`runtime/stremio-core-runtime.ts`: `dispatchPlayerAction`,
//! `flushPlaybackProgress`, `PROGRESS_SYNC_INTERVAL_SECONDS`). The core owns
//! library-item creation, the watched threshold, local persistence through the
//! Env storage and the network sync. Like the TS runtime, every progress call is
//! a no-op while signed out: nothing is recorded locally or remotely, and
//! `resume` reports nothing. All calls only dispatch core actions; none block.

use std::sync::MutexGuard;

use serde_json::json;
use stremio_core::{
    models::player::Selected,
    runtime::msg::{Action, ActionCtx, ActionLoad, ActionPlayer},
    types::{
        addon::{ResourcePath, ResourceRequest},
        resource::{MetaItemPreview, Stream},
    },
};
use url::Url;

use super::{CoreError, CoreErrorKind, CoreSession, model::CoreModelField, session::Running};

/// Playback time between pushes to the core (`PROGRESS_SYNC_INTERVAL_SECONDS`).
const SYNC_INTERVAL_SECS: f64 = 10.0;
/// `normalizeResumeState`: offset at least this far in...
const RESUME_MIN_OFFSET_SECS: f64 = 30.0;
/// ...and at least this much left.
const RESUME_MIN_REMAINING_SECS: f64 = 60.0;
const DEVICE: &str = "Panorama";

/// Where a film can be resumed from; only produced when it is worth resuming.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResumePoint {
    /// Saved position in seconds.
    pub offset_secs: f64,
    /// Saved duration in seconds.
    pub duration_secs: f64,
}

/// One position report from the video player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackSample {
    /// Current position in seconds.
    pub time_secs: f64,
    /// Total duration in seconds; reports with no duration yet are not pushed.
    pub duration_secs: f64,
    /// Whether playback is paused.
    pub paused: bool,
}

/// The film fields the watchlist and library item are created from.
#[derive(Clone, Debug, PartialEq)]
pub struct FilmMeta {
    /// Opaque film ID, as used by the addons.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Poster image, if known.
    pub poster: Option<Url>,
}

impl FilmMeta {
    fn preview(&self) -> Result<MetaItemPreview, CoreError> {
        serde_json::from_value(json!({
            "id": self.id, "type": "movie", "name": self.name, "poster": self.poster,
        }))
        .map_err(|_| CoreErrorKind::Other.into())
    }
}

/// A film and the core stream about to be played.
#[derive(Clone, Debug)]
pub struct PlaybackTarget {
    /// The film being played.
    pub film: FilmMeta,
    /// The chosen stream, as supplied by the addons module.
    pub stream: Stream,
    /// Addon that supplies the film's metadata; the core loads it to create the library item.
    pub meta_base: Url,
    /// Addon that supplied the stream.
    pub stream_base: Url,
}

/// Per-playback throttle state; reset whenever a playback begins or stops.
#[derive(Default)]
pub(super) struct Tracker {
    film: Option<String>,
    last_sync: f64,
    paused: Option<bool>,
    last: Option<PlaybackSample>,
}

fn millis(secs: f64) -> u64 {
    (secs * 1000.0).round().max(0.0) as u64
}

impl Running {
    fn tracker(&self) -> MutexGuard<'_, Tracker> {
        self.tracker.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn player(&self, action: ActionPlayer) {
        self.dispatch_to(Some(CoreModelField::Player), Action::Player(action));
    }

    fn time_action(&self, seek: bool, time_secs: f64, duration_secs: f64) {
        let (time, duration, device) = (millis(time_secs), millis(duration_secs), DEVICE.into());
        self.player(if seek {
            ActionPlayer::Seek {
                time,
                duration,
                device,
            }
        } else {
            ActionPlayer::TimeChanged {
                time,
                duration,
                device,
            }
        });
    }

    /// `flushPlaybackProgress`: only with a finite positive duration.
    fn flush(&self, tracker: &mut Tracker, sample: PlaybackSample) {
        if sample.time_secs.is_finite()
            && sample.duration_secs.is_finite()
            && sample.duration_secs > 0.0
        {
            self.time_action(false, sample.time_secs, sample.duration_secs);
            tracker.last_sync = sample.time_secs;
        }
    }
}

impl CoreSession {
    fn signed_in_running(&self) -> Result<&Running, CoreError> {
        match self.running() {
            Some(running) if self.is_signed_in() => Ok(running),
            _ => Err(CoreErrorKind::NotSignedIn.into()),
        }
    }

    /// Returns the saved resume point when the position is at least 30 s in with
    /// at least 60 s left (`normalizeResumeState`); `None` while signed out.
    pub fn resume(&self, film_id: &str) -> Option<ResumePoint> {
        let running = self.running().filter(|_| self.is_signed_in())?;
        let model = running.runtime.model().unwrap_or_else(|p| p.into_inner());
        let state = &model.ctx.inner.library.items.get(film_id)?.state;
        let (offset_secs, duration_secs) = (
            state.time_offset as f64 / 1000.0,
            state.duration as f64 / 1000.0,
        );
        (offset_secs >= RESUME_MIN_OFFSET_SECS
            && duration_secs - offset_secs >= RESUME_MIN_REMAINING_SECS)
            .then_some(ResumePoint {
                offset_secs,
                duration_secs,
            })
    }

    /// Whether the film is saved to the watchlist (neither removed nor temporary).
    /// `false` while signed out.
    pub fn in_library(&self, film_id: &str) -> bool {
        let Some(running) = self.running().filter(|_| self.is_signed_in()) else {
            return false;
        };
        let model = running.runtime.model().unwrap_or_else(|p| p.into_inner());
        model
            .ctx
            .inner
            .library
            .items
            .get(film_id)
            .is_some_and(|item| !item.removed && !item.temp)
    }

    /// Adds or removes the film from the watchlist (`setWatchlisted`). The change
    /// lands asynchronously; observe `CoreChange::LibraryChanged`.
    pub fn set_in_library(&self, film: &FilmMeta, saved: bool) -> Result<(), CoreError> {
        let running = self.signed_in_running()?;
        running.dispatch_to(
            None,
            Action::Ctx(if saved {
                ActionCtx::AddToLibrary(film.preview()?)
            } else {
                ActionCtx::RemoveFromLibrary(film.id.clone())
            }),
        );
        Ok(())
    }

    /// Loads the film into the core's player so progress can be reported
    /// (`beginPlayback`). `start_secs` is where playback starts (a resume offset or 0).
    pub fn begin_playback(&self, target: PlaybackTarget, start_secs: f64) -> Result<(), CoreError> {
        let running = self.signed_in_running()?;
        let id = &target.film.id;
        let request = |base: &Url, resource| {
            ResourceRequest::new(
                base.clone(),
                ResourcePath::without_extra(resource, "movie", id),
            )
        };
        let selected = Selected {
            stream: target.stream.clone(),
            stream_request: Some(request(&target.stream_base, "stream")),
            meta_request: Some(request(&target.meta_base, "meta")),
            subtitles_path: None,
        };
        let mut tracker = running.tracker();
        *tracker = Tracker {
            film: Some(id.clone()),
            last_sync: if start_secs.is_finite() {
                start_secs.max(0.0)
            } else {
                0.0
            },
            ..Tracker::default()
        };
        running.dispatch_to(
            Some(CoreModelField::Player),
            Action::Load(ActionLoad::Player(Box::new(selected))),
        );
        Ok(())
    }

    /// Reports the playback position. Pushes to the core when the pause state
    /// changes, or the position moved 10 s or more since the last push.
    pub fn report_progress(&self, film_id: &str, sample: PlaybackSample) {
        let Ok(running) = self.signed_in_running() else {
            return;
        };
        let mut tracker = running.tracker();
        if tracker.film.as_deref() != Some(film_id) {
            return;
        }
        tracker.last = Some(sample);
        let previous = tracker.paused.replace(sample.paused);
        let mut flushed = false;
        if previous != Some(sample.paused) {
            running.player(ActionPlayer::PausedChanged {
                paused: sample.paused,
            });
            // The first sample only announces the state; later changes are pauses/resumes.
            if previous.is_some() {
                running.flush(&mut tracker, sample);
                flushed = true;
            }
        }
        if !flushed && (sample.time_secs - tracker.last_sync).abs() >= SYNC_INTERVAL_SECS {
            running.flush(&mut tracker, sample);
        }
    }

    /// Reports a user seek (`seekPlayback`): the core records the new position
    /// at once and the 10 s window restarts from it.
    pub fn report_seek(&self, film_id: &str, sample: PlaybackSample) {
        let Ok(running) = self.signed_in_running() else {
            return;
        };
        let mut tracker = running.tracker();
        if tracker.film.as_deref() != Some(film_id) || !sample.time_secs.is_finite() {
            return;
        }
        let time = sample.time_secs.max(0.0);
        tracker.last = Some(PlaybackSample {
            time_secs: time,
            ..sample
        });
        running.time_action(true, time, sample.duration_secs.max(0.0));
        tracker.last_sync = time;
    }

    /// The video reached its end: pushes the final position at the duration so
    /// the core's watched threshold marks the film watched, then sends `Ended`
    /// (the TS runtime's `ended` handler). Call `stop_playback` afterwards.
    pub fn finish(&self, film_id: &str) {
        let Ok(running) = self.signed_in_running() else {
            return;
        };
        let mut tracker = running.tracker();
        let Some(mut last) = tracker
            .last
            .filter(|_| tracker.film.as_deref() == Some(film_id))
        else {
            return;
        };
        last.time_secs = last.duration_secs;
        running.flush(&mut tracker, last);
        tracker.last = Some(last);
        running.player(ActionPlayer::Ended);
    }

    /// Stops playback (`stopPlayback`): flushes the last position, then unloads
    /// the player, which makes the core push the library item.
    pub fn stop_playback(&self, film_id: &str) {
        let Ok(running) = self.signed_in_running() else {
            return;
        };
        let mut tracker = running.tracker();
        if tracker.film.as_deref() != Some(film_id) {
            return;
        }
        if let Some(last) = tracker.last {
            running.flush(&mut tracker, last);
        }
        *tracker = Tracker::default();
        running.dispatch_to(Some(CoreModelField::Player), Action::Unload);
    }
}
