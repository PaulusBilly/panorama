use super::*;

#[tokio::test]
async fn end_file_eof_still_finishes_progress_once() {
    let (mut runner, backend, progress) = setup();
    lock(&backend.scripts).push_back(Script {
        events: vec![
            PlayerEvent::Duration(Some(600.0)),
            PlayerEvent::TimePos(Some(599.0)),
            PlayerEvent::EndFile {
                reason: panorama_mpv::EndReason::Eof,
                playlist_entry_id: 1,
            },
            PlayerEvent::EndFile {
                reason: panorama_mpv::EndReason::Eof,
                playlist_entry_id: 1,
            },
        ],
        ..Default::default()
    });
    runner
        .control(Command::Headless(Box::new(selection())))
        .await;
    resolve(&mut runner).await;
    assert_eq!(runner.model.status, Status::Ended);
    assert_eq!(*lock(&progress.calls), ["begin:0", "sample:599", "finish"]);
}

#[tokio::test]
async fn stop_before_resume_seek_lands_never_reports_zero() {
    let (mut runner, backend, progress) = setup();
    lock(&backend.scripts).push_back(Script {
        events: vec![
            PlayerEvent::Duration(Some(600.0)),
            PlayerEvent::TimePos(Some(0.0)),
        ],
        ..Default::default()
    });
    let mut selection = selection();
    selection.start = StartMode::Resume(90.0);
    runner.control(Command::Headless(Box::new(selection))).await;
    resolve(&mut runner).await;
    runner.control(Command::Stop).await;
    assert_eq!(*lock(&progress.calls), ["begin:90", "stop"]);
}

#[tokio::test]
async fn retry_before_resume_seek_lands_keeps_target() {
    let (mut runner, backend, progress) = setup();
    lock(&backend.scripts).extend([
        Script {
            events: vec![
                PlayerEvent::Duration(Some(600.0)),
                PlayerEvent::TimePos(Some(0.0)),
                PlayerEvent::Error(panorama_mpv::MpvError::Loading),
            ],
            ..Default::default()
        },
        Script {
            events: vec![
                PlayerEvent::Duration(Some(600.0)),
                PlayerEvent::TimePos(Some(0.0)),
                PlayerEvent::TimePos(Some(90.0)),
            ],
            ..Default::default()
        },
    ]);
    let mut selection = selection();
    selection.start = StartMode::Resume(90.0);
    runner.control(Command::Headless(Box::new(selection))).await;
    resolve(&mut runner).await;
    runner.control(Command::Retry).await;
    resolve(&mut runner).await;
    assert_eq!(
        lock(&backend.log)
            .iter()
            .filter(|entry| *entry == "load@90")
            .count(),
        2
    );
    assert_eq!(
        *lock(&progress.calls),
        ["begin:90", "stop", "begin:90", "sample:90"]
    );
}

#[tokio::test]
async fn user_seek_releases_resume_sample_guard() {
    let (mut runner, backend, progress) = setup();
    lock(&backend.scripts).push_back(Script {
        events: vec![
            PlayerEvent::Duration(Some(600.0)),
            PlayerEvent::TimePos(Some(0.0)),
        ],
        ..Default::default()
    });
    let mut selection = selection();
    selection.start = StartMode::Resume(90.0);
    runner.control(Command::Headless(Box::new(selection))).await;
    resolve(&mut runner).await;
    runner.control(Command::Seek(30.0)).await;
    assert_eq!(*lock(&progress.calls), ["begin:90", "seek:30"]);
}

#[tokio::test]
async fn resume_confirmation_accepts_three_seconds_either_side() {
    for confirmed in [87.0, 93.0] {
        let (mut runner, backend, progress) = setup();
        lock(&backend.scripts).push_back(Script {
            events: vec![
                PlayerEvent::Duration(Some(600.0)),
                PlayerEvent::TimePos(Some(0.0)),
                PlayerEvent::TimePos(Some(86.9)),
                PlayerEvent::TimePos(Some(confirmed)),
            ],
            ..Default::default()
        });
        let mut selection = selection();
        selection.start = StartMode::Resume(90.0);
        runner.control(Command::Headless(Box::new(selection))).await;
        resolve(&mut runner).await;
        assert_eq!(
            *lock(&progress.calls),
            ["begin:90".to_owned(), format!("sample:{confirmed}")]
        );
    }
}

#[tokio::test]
async fn a_dying_stream_jumping_to_the_end_does_not_move_the_resume_point() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).extend([
        Script {
            events: vec![
                PlayerEvent::Duration(Some(600.0)),
                PlayerEvent::TimePos(Some(100.0)),
                PlayerEvent::TimePos(Some(100.5)),
                // A read error ends the file, and mpv calls the position its end.
                PlayerEvent::TimePos(Some(600.0)),
                PlayerEvent::Error(panorama_mpv::MpvError::Loading),
            ],
            ..Default::default()
        },
        Script::default(),
    ]);
    let mut selection = selection();
    selection.start = StartMode::Resume(100.0);
    runner.control(Command::Headless(Box::new(selection))).await;
    resolve(&mut runner).await;
    runner.control(Command::Retry).await;
    resolve(&mut runner).await;
    assert!(
        lock(&backend.log).iter().any(|entry| entry == "load@100.5"),
        "{:?}",
        lock(&backend.log)
    );
}

#[tokio::test]
async fn a_position_from_before_a_seek_does_not_move_the_resume_point() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).extend([
        Script {
            events: vec![
                PlayerEvent::Duration(Some(600.0)),
                PlayerEvent::TimePos(Some(100.0)),
                PlayerEvent::TimePos(Some(100.5)),
            ],
            ..Default::default()
        },
        Script::default(),
    ]);
    let mut selection = selection();
    selection.start = StartMode::Resume(100.0);
    runner.control(Command::Headless(Box::new(selection))).await;
    let (generation, result) = runner.pending.next().await.unwrap();
    runner.opened(generation, result).await;
    runner.control(Command::Seek(300.0)).await;
    // Positions mpv sent before it heard of the seek.
    runner.tick().await;
    runner.control(Command::Retry).await;
    resolve(&mut runner).await;
    assert!(
        lock(&backend.log).iter().any(|entry| entry == "load@300"),
        "{:?}",
        lock(&backend.log)
    );
}

#[tokio::test]
async fn pausing_mid_seek_does_not_record_where_it_was() {
    let (mut runner, backend, progress) = setup();
    lock(&backend.scripts).push_back(Script {
        events: vec![
            PlayerEvent::Duration(Some(600.0)),
            PlayerEvent::TimePos(Some(100.0)),
        ],
        ..Default::default()
    });
    let mut selection = selection();
    selection.start = StartMode::Resume(100.0);
    runner.control(Command::Headless(Box::new(selection))).await;
    resolve(&mut runner).await;
    runner.control(Command::Seek(300.0)).await;
    // A position mpv sent before it heard of the seek, then a pause.
    runner
        .model
        .apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(100.5))));
    runner.sample(false).await;
    let calls = lock(&progress.calls).clone();
    assert!(calls.iter().all(|call| call != "sample:100.5"), "{calls:?}");
}
