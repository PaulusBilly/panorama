use super::*;

fn events() -> Vec<HostEvent> {
    let mut events = vec![
        HostEvent::Resolving,
        HostEvent::Loading,
        HostEvent::Failed(PlaybackError::Network),
        HostEvent::SourceChanged(SourceRef {
            group: 1,
            stream: 2,
        }),
        HostEvent::Closed,
        HostEvent::Volume(255),
        HostEvent::Muted(true),
        HostEvent::Seeked(0.0),
        HostEvent::Mpv(PlayerEvent::Ready),
        HostEvent::Mpv(PlayerEvent::TimePos(Some(4.0))),
        HostEvent::Mpv(PlayerEvent::TimePos(None)),
        HostEvent::Mpv(PlayerEvent::Duration(Some(100.0))),
        HostEvent::Mpv(PlayerEvent::Duration(None)),
        HostEvent::Mpv(PlayerEvent::Pause(true)),
        HostEvent::Mpv(PlayerEvent::EofReached(false)),
        HostEvent::Mpv(PlayerEvent::EofReached(true)),
        HostEvent::Mpv(PlayerEvent::PausedForCache(true)),
        HostEvent::Mpv(PlayerEvent::PausedForCache(false)),
        HostEvent::Mpv(PlayerEvent::CacheBufferingState(Some(50.0))),
        HostEvent::Mpv(PlayerEvent::DemuxerCacheState(panorama_mpv::CacheState {
            cache_end_seconds: Some(8.0),
            ..Default::default()
        })),
        HostEvent::Mpv(PlayerEvent::HwdecCurrent(Some("d3d11va".into()))),
        HostEvent::Mpv(PlayerEvent::TrackList(vec![])),
        HostEvent::Mpv(PlayerEvent::AudioDeviceList(vec![])),
        HostEvent::Mpv(PlayerEvent::SubText(Some(
            "https://fixture.invalid/private-token".into(),
        ))),
        HostEvent::Mpv(PlayerEvent::SubStart(Some(1.0))),
        HostEvent::Mpv(PlayerEvent::SubEnd(Some(2.0))),
        HostEvent::Mpv(PlayerEvent::EndFile {
            reason: EndReason::Eof,
            playlist_entry_id: 1,
        }),
        HostEvent::Mpv(PlayerEvent::EndFile {
            reason: EndReason::Error(MpvError::Demuxer),
            playlist_entry_id: 1,
        }),
        HostEvent::Mpv(PlayerEvent::Error(MpvError::Loading)),
        HostEvent::Mpv(PlayerEvent::Shutdown),
    ];
    for reason in [
        EndReason::Stop,
        EndReason::Quit,
        EndReason::Redirect,
        EndReason::Other,
    ] {
        events.push(HostEvent::Mpv(PlayerEvent::EndFile {
            reason,
            playlist_entry_id: 1,
        }));
    }
    events
}

#[test]
fn reducer_event_status_table() {
    for status in [
        Status::Idle,
        Status::Preparing(Stage::ResolvingSource),
        Status::Preparing(Stage::LoadingVideo),
        Status::Ready,
        Status::Buffering,
        Status::Ended,
        Status::Error(PlaybackError::Network),
    ] {
        for event in events() {
            let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
            model.status = status;
            model.known_file = !matches!(
                status,
                Status::Idle | Status::Preparing(Stage::ResolvingSource)
            );
            model.has_reached_playable =
                matches!(status, Status::Ready | Status::Buffering | Status::Ended);
            let was_playable = model.has_reached_playable;
            let live = model.known_file && !matches!(status, Status::Idle | Status::Error(_));
            let expected = match &event {
                HostEvent::Closed => Status::Idle,
                HostEvent::Resolving => Status::Preparing(Stage::ResolvingSource),
                HostEvent::Loading => Status::Preparing(Stage::LoadingVideo),
                HostEvent::Failed(error) => Status::Error(*error),
                HostEvent::Mpv(PlayerEvent::Error(error))
                | HostEvent::Mpv(PlayerEvent::EndFile {
                    reason: EndReason::Error(error),
                    ..
                }) => Status::Error((*error).into()),
                HostEvent::Seeked(_) if live && status == Status::Ended => Status::Ready,
                HostEvent::Mpv(PlayerEvent::TimePos(Some(_)))
                    if live && matches!(status, Status::Preparing(_)) =>
                {
                    Status::Ready
                }
                HostEvent::Mpv(PlayerEvent::PausedForCache(true))
                    if live && status != Status::Ended =>
                {
                    Status::Buffering
                }
                HostEvent::Mpv(PlayerEvent::PausedForCache(false))
                    if live && status != Status::Ended && was_playable =>
                {
                    Status::Ready
                }
                HostEvent::Mpv(PlayerEvent::EofReached(true))
                | HostEvent::Mpv(PlayerEvent::EndFile {
                    reason: EndReason::Eof,
                    ..
                }) if live => Status::Ended,
                _ => status,
            };
            let closed = event == HostEvent::Closed;
            model.apply(event.clone());
            assert_eq!(model.status, expected, "{status:?} + {event:?}");
            assert_eq!(
                model.has_reached_playable,
                (!closed && was_playable)
                    || matches!(expected, Status::Ready | Status::Buffering | Status::Ended)
            );
            assert!(model.volume <= 100);
        }
    }
}

#[test]
fn readiness_latches_across_retries_and_sources() {
    let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
    model.apply(HostEvent::Resolving);
    model.apply(HostEvent::Mpv(PlayerEvent::Ready));
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(1.0))));
    assert!(model.initial_preparing());
    model.apply(HostEvent::Loading);
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(false)));
    assert!(model.initial_preparing());
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(1.0))));
    assert_eq!(model.status, Status::Ready);
    model.apply(HostEvent::Resolving);
    assert!(!model.initial_preparing());
    model.apply(HostEvent::Closed);
    assert!(!model.has_reached_playable);
}

#[test]
fn track_labels_and_cache_measurements() {
    let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
    let track = |id, title, lang, kind| panorama_mpv::Track {
        id,
        title,
        lang,
        codec: Some("aac".into()),
        kind,
        default: false,
        forced: false,
        external: false,
        hearing_impaired: false,
        selected: id == 1,
    };
    model.apply(HostEvent::Mpv(PlayerEvent::TrackList(vec![
        track(
            1,
            Some("Commentary".into()),
            Some("eng".into()),
            TrackKind::Audio,
        ),
        track(2, None, Some("fra".into()), TrackKind::Audio),
        track(3, None, None, TrackKind::Subtitle),
        track(4, None, None, TrackKind::Video),
    ])));
    assert_eq!(model.audio[0].label, "Commentary");
    assert_eq!(model.audio[1].label, "French");
    assert_eq!(model.subtitles[0].label, "Track 1");
    assert_eq!(model.audio[0].description.as_deref(), Some("aac"));
    model.apply(HostEvent::Mpv(PlayerEvent::DemuxerCacheState(
        panorama_mpv::CacheState {
            cache_end_seconds: Some(123.0),
            ..Default::default()
        },
    )));
    assert_eq!(model.buffered_until, Some(123.0));
    for value in [f64::NAN, f64::INFINITY, -1.0] {
        model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(value))));
        model.apply(HostEvent::Mpv(PlayerEvent::Duration(Some(value))));
        assert!(model.time.is_none() && model.duration.is_none());
    }
}

#[test]
fn every_event_and_error_debug_is_secret_free() {
    for event in events() {
        let debug = format!("{event:?}");
        assert!(!debug.contains("https://fixture.invalid"));
        assert!(!debug.contains("private-token"));
    }
    for error in [
        PlaybackError::LibraryMissing,
        PlaybackError::SourceUnavailable,
        PlaybackError::SourceExpired,
        PlaybackError::NoPeers,
        PlaybackError::NoVideoFile,
        PlaybackError::Network,
        PlaybackError::Decode,
        PlaybackError::Unknown,
    ] {
        let text = format!("{error:?} {}", error.message());
        assert!(!text.contains("https://fixture.invalid"));
        assert!(!text.contains("private-token"));
    }
}

#[test]
fn eof_stays_ended_until_explicit_replay_seek() {
    let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
    model.apply(HostEvent::Loading);
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(5.0))));
    model.apply(HostEvent::Mpv(PlayerEvent::EndFile {
        reason: EndReason::Eof,
        playlist_entry_id: 1,
    }));
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(600.0))));
    assert_eq!(model.status, Status::Ended);
    model.apply(HostEvent::Seeked(0.0));
    assert_eq!(model.status, Status::Ready);
}

#[test]
fn keep_open_eof_rearms_after_false_and_replay_clears_ended() {
    let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
    model.apply(HostEvent::Loading);
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(5.0))));
    model.apply(HostEvent::Mpv(PlayerEvent::EofReached(false)));
    model.apply(HostEvent::Mpv(PlayerEvent::EofReached(true)));
    assert_eq!(model.status, Status::Ended);
    model.apply(HostEvent::Seeked(0.0));
    model.apply(HostEvent::Mpv(PlayerEvent::Pause(false)));
    model.apply(HostEvent::Mpv(PlayerEvent::EofReached(true)));
    assert_eq!(model.status, Status::Ready);
    assert!(!model.paused);
    model.apply(HostEvent::Mpv(PlayerEvent::EofReached(false)));
    model.apply(HostEvent::Mpv(PlayerEvent::EofReached(true)));
    assert_eq!(model.status, Status::Ended);
}

fn playing_model() -> PlaybackModel {
    let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
    model.apply(HostEvent::Loading);
    for time in [1.0, 2.0, 3.0] {
        model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(time))));
    }
    model
}

#[test]
fn a_seek_induced_cache_pause_is_not_a_rebuffer() {
    let mut model = playing_model();
    model.apply(HostEvent::Seeked(600.0));
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(false)));
    assert_eq!(model.rebuffers, 0);
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(601.0))));
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
    assert_eq!(model.rebuffers, 1);
}

#[test]
fn the_first_buffering_of_a_switched_source_is_not_a_rebuffer() {
    let mut model = playing_model();
    model.apply(HostEvent::Resolving);
    model.apply(HostEvent::Loading);
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(false)));
    assert_eq!(model.rebuffers, 0);
}

#[test]
fn a_real_mid_play_stall_is_a_rebuffer_but_not_while_paused() {
    let mut model = playing_model();
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
    assert_eq!(model.rebuffers, 1);
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(false)));
    model.apply(HostEvent::Mpv(PlayerEvent::Pause(true)));
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
    assert_eq!(model.rebuffers, 1);
}

#[test]
fn rebuffers_count_stalls_after_the_first_playable_frame() {
    let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
    model.apply(HostEvent::Loading);
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
    assert_eq!(model.rebuffers, 0);
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(false)));
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(3.0))));
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(4.0))));
    for _ in 0..2 {
        model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
        model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
        model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(false)));
    }
    assert_eq!(model.rebuffers, 2);
    model.apply(HostEvent::Mpv(PlayerEvent::DemuxerCacheState(
        panorama_mpv::CacheState {
            buffered_seconds: Some(4.5),
            cache_end_seconds: Some(9.0),
            ..Default::default()
        },
    )));
    assert_eq!(
        (model.cache_seconds, model.buffered_until),
        (Some(4.5), Some(9.0))
    );
    model.apply(HostEvent::Resolving);
    assert_eq!((model.rebuffers, model.cache_seconds), (0, None));
}
