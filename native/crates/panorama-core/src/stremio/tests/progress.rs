use std::time::Duration;

use serde_json::json;
use stremio_core::types::resource::Stream;
use url::Url;

use super::{
    TestEnv,
    mock::{EMAIL, MockApi, PASSWORD},
    tls::TlsMock,
};
use crate::stremio::{
    CoreErrorKind, CoreSession, FilmMeta, PlaybackSample, PlaybackTarget, ResumePoint,
};

const FILM: &str = "tt0816692";

fn film() -> FilmMeta {
    FilmMeta {
        id: FILM.into(),
        name: "Interstellar".into(),
        poster: None,
    }
}

fn sample(time_secs: f64, paused: bool) -> PlaybackSample {
    PlaybackSample {
        time_secs,
        duration_secs: 7200.0,
        paused,
    }
}

fn target(meta_base: &str) -> PlaybackTarget {
    PlaybackTarget {
        film: film(),
        stream: serde_json::from_value::<Stream>(
            json!({"url": "https://video.example.test/a.mp4"}),
        )
        .unwrap(),
        meta_base: meta_base.parse().unwrap(),
        stream_base: "https://streams.example.test/manifest.json"
            .parse()
            .unwrap(),
    }
}

/// A meta addon that is unreachable; the library item must already exist.
const OFFLINE_META: &str = "https://127.0.0.1:1/manifest.json";

async fn signed_in(api: &MockApi) -> CoreSession {
    let mut session = CoreSession::start().await.unwrap();
    session
        .sign_in(EMAIL.into(), PASSWORD.into())
        .await
        .unwrap();
    let _ = api;
    session
}

/// Polls until `condition` holds; the bound is a hang guard, not a pacing sleep.
async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

/// Offset held by the account library (updated asynchronously after a push).
fn library_offset_ms(session: &CoreSession) -> u64 {
    let model = session.running().unwrap().runtime.model().unwrap();
    model.ctx.inner.library.items[FILM].state.time_offset
}

/// Waits until the core pushed one more library write to the API.
async fn pushed(api: &MockApi, before: usize) {
    until(|| put_calls(api) > before).await;
}

/// Position the core's player item holds (updated on every dispatched action).
fn player_offset_ms(session: &CoreSession) -> u64 {
    let model = session.running().unwrap().runtime.model().unwrap();
    model
        .player
        .library_item
        .as_ref()
        .unwrap()
        .state
        .time_offset
}

fn put_calls(api: &MockApi) -> usize {
    api.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|path| *path == "/api/datastorePut")
        .count()
}

async fn saved_film(session: &CoreSession, api: &MockApi) {
    session.set_in_library(&film(), true).unwrap();
    until(|| session.in_library(FILM) && put_calls(api) >= 1).await;
}

#[test]
fn watchlist_adds_and_removes_through_the_core_and_requires_an_account() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        assert_eq!(
            session.set_in_library(&film(), true).unwrap_err().kind,
            CoreErrorKind::NotSignedIn
        );
        assert!(!session.in_library(FILM));
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(!session.in_library(FILM));
        saved_film(&session, &api).await;
        assert!(session.in_library(FILM));
        session.set_in_library(&film(), false).unwrap();
        until(|| !session.in_library(FILM)).await;
        until(|| put_calls(&api) >= 2).await;
    });
}

#[test]
fn progress_is_ignored_while_signed_out() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let session = CoreSession::start().await.unwrap();
        assert_eq!(
            session
                .begin_playback(target(OFFLINE_META), 0.0)
                .unwrap_err()
                .kind,
            CoreErrorKind::NotSignedIn
        );
        session.report_progress(FILM, sample(500.0, false));
        session.stop_playback(FILM);
        assert_eq!(session.resume(FILM), None);
        assert_eq!(put_calls(&api), 0);
    });
}

#[test]
fn progress_syncs_every_ten_seconds_and_on_pause_and_seek() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let session = signed_in(&api).await;
        saved_film(&session, &api).await;
        session.begin_playback(target(OFFLINE_META), 0.0).unwrap();
        session.report_progress(FILM, sample(5.0, false));
        assert_eq!(player_offset_ms(&session), 0);
        session.report_progress(FILM, sample(9.9, false));
        assert_eq!(player_offset_ms(&session), 0);
        session.report_progress(FILM, sample(10.0, false));
        assert_eq!(player_offset_ms(&session), 10_000);
        session.report_progress(FILM, sample(19.0, false));
        assert_eq!(player_offset_ms(&session), 10_000);
        session.report_progress(FILM, sample(20.5, false));
        assert_eq!(player_offset_ms(&session), 20_500);
        session.report_progress(FILM, sample(23.0, true));
        assert_eq!(player_offset_ms(&session), 23_000);
        session.report_progress(FILM, sample(24.0, true));
        assert_eq!(player_offset_ms(&session), 23_000);
        // A user seek records the position at once and restarts the 10 s window there.
        session.report_seek(FILM, sample(3000.0, true));
        assert_eq!(player_offset_ms(&session), 3_000_000);
        session.report_progress(FILM, sample(3009.0, false));
        assert_eq!(player_offset_ms(&session), 3_009_000);
        session.report_progress(FILM, sample(3015.0, false));
        assert_eq!(player_offset_ms(&session), 3_009_000);
        session.report_progress(FILM, sample(3019.5, false));
        assert_eq!(player_offset_ms(&session), 3_019_500);
    });
}

#[test]
fn reports_without_a_duration_or_for_another_film_are_not_pushed() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let session = signed_in(&api).await;
        saved_film(&session, &api).await;
        session.begin_playback(target(OFFLINE_META), 0.0).unwrap();
        session.report_progress(FILM, sample(1.0, false));
        let unknown = PlaybackSample {
            duration_secs: 0.0,
            ..sample(60.0, false)
        };
        session.report_progress(FILM, unknown);
        assert_eq!(player_offset_ms(&session), 0);
        session.report_progress("tt0000001", sample(100.0, false));
        assert_eq!(player_offset_ms(&session), 0);
        session.stop_playback("tt0000001");
        assert_eq!(player_offset_ms(&session), 0);
    });
}

#[test]
fn stop_flushes_and_pushes_the_library_item_so_resume_applies_only_when_meaningful() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let session = signed_in(&api).await;
        saved_film(&session, &api).await;
        assert_eq!(session.resume(FILM), None);
        let puts = put_calls(&api);
        session.begin_playback(target(OFFLINE_META), 0.0).unwrap();
        session.report_progress(FILM, sample(1.0, false));
        session.report_progress(FILM, sample(1800.0, false));
        session.stop_playback(FILM);
        pushed(&api, puts).await;
        until(|| library_offset_ms(&session) == 1_800_000).await;
        assert_eq!(
            session.resume(FILM),
            Some(ResumePoint {
                offset_secs: 1800.0,
                duration_secs: 7200.0
            })
        );
        // Less than 30 s in, or less than 60 s left, is not worth resuming.
        // The second offset is past 90 %, which the core resets to 0 when unloading.
        for (seek, stored) in [(29.0, 29_000), (7141.0, 0)] {
            let puts = put_calls(&api);
            session.begin_playback(target(OFFLINE_META), 0.0).unwrap();
            session.report_progress(FILM, sample(1.0, false));
            session.report_seek(FILM, sample(seek, false));
            session.stop_playback(FILM);
            pushed(&api, puts).await;
            until(|| library_offset_ms(&session) == stored).await;
            assert_eq!(session.resume(FILM), None);
        }
    });
}

#[test]
fn finishing_near_the_end_marks_the_film_watched_and_clears_resume() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let session = signed_in(&api).await;
        saved_film(&session, &api).await;
        session.begin_playback(target(OFFLINE_META), 0.0).unwrap();
        // The core's first push only records which video is playing; later pushes accumulate.
        for second in [1.0, 100.0, 4000.0, 7100.0] {
            session.report_progress(FILM, sample(second, false));
        }
        let puts = put_calls(&api);
        session.finish(FILM);
        session.stop_playback(FILM);
        pushed(&api, puts).await;
        until(|| {
            let model = session.running().unwrap().runtime.model().unwrap();
            model.ctx.inner.library.items[FILM].state.times_watched == 1
        })
        .await;
        let model = session.running().unwrap().runtime.model().unwrap();
        assert_eq!(model.ctx.inner.library.items[FILM].state.flagged_watched, 1);
        drop(model);
        assert_eq!(session.resume(FILM), None);
        assert!(session.in_library(FILM));
    });
}

#[test]
fn library_item_is_created_from_the_films_meta_when_needed() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let server = fixture.runtime.block_on(TlsMock::start());
    fixture.trust_tls(&[&server]);
    *server.body.lock().unwrap() = serde_json::to_vec(&json!({"meta": {
        "id": FILM, "type": "movie", "name": "Interstellar"
    }}))
    .unwrap();
    fixture.runtime.block_on(async {
        let session = signed_in(&api).await;
        let meta: Url = server.base.join("manifest.json").unwrap();
        session.begin_playback(target(meta.as_str()), 0.0).unwrap();
        until(|| {
            let model = session.running().unwrap().runtime.model().unwrap();
            model.player.library_item.is_some()
        })
        .await;
        session.report_progress(FILM, sample(1.0, false));
        session.report_progress(FILM, sample(900.0, false));
        let puts = put_calls(&api);
        session.stop_playback(FILM);
        pushed(&api, puts).await;
        until(|| session.resume(FILM).is_some()).await;
        // Played but never saved: it resumes, yet stays off the watchlist.
        assert_eq!(session.resume(FILM).map(|r| r.offset_secs), Some(900.0));
        assert!(!session.in_library(FILM));
        assert!(server.requests.lock().unwrap()[0].starts_with("GET /meta/movie/tt0816692.json "));
    });
}
