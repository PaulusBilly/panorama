use super::super::{
    fake::Script,
    model::{PlaybackError, Stage},
};
use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct RecordingProgress {
    calls: Mutex<Vec<String>>,
}
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
impl ProgressSink for RecordingProgress {
    fn send(&self, event: Progress) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            lock(&self.calls).push(match event {
                Progress::Begin(_, seconds) => format!("begin:{seconds}"),
                Progress::Sample(_, sample) => format!("sample:{}", sample.time_secs),
                Progress::Seek(_, sample) => format!("seek:{}", sample.time_secs),
                Progress::Finish(_) => "finish".into(),
                Progress::Stop(_) => "stop".into(),
            });
        })
    }
    fn presence(&self, _: &PlaybackSelection, _: &PlaybackModel) {}
    fn clear(&self) {}
}
fn selection() -> PlaybackSelection {
    let panorama_core::addons::StreamsEvent::Groups(mut groups) = crate::fixtures::streams(false)
    else {
        panic!("fixture");
    };
    if let panorama_core::addons::StreamState::Ready(streams) = &mut groups[0].state {
        streams.push(streams[0].clone());
    }
    PlaybackSelection {
        film: crate::fixtures::details("tmdb:100").unwrap(),
        groups,
        source: SourceRef::default(),
        start: StartMode::Restart,
        meta_base: None,
    }
}
fn setup() -> (Runner, Arc<FakeBackend>, Arc<RecordingProgress>) {
    let backend = Arc::new(FakeBackend::new(None));
    let progress = Arc::new(RecordingProgress::default());
    let (events, _) = mpsc::unbounded();
    (
        Runner::new(backend.clone(), progress.clone(), events),
        backend,
        progress,
    )
}
async fn resolve(runner: &mut Runner) {
    let (generation, result) = runner.pending.next().await.unwrap();
    runner.opened(generation, result).await;
    runner.tick().await;
}
#[tokio::test]
async fn start_ready_ended_and_progress_order() {
    let (mut runner, backend, progress) = setup();
    lock(&backend.scripts).push_back(Script {
        events: vec![
            PlayerEvent::Duration(Some(600.0)),
            PlayerEvent::TimePos(Some(1.0)),
            PlayerEvent::TimePos(Some(12.0)),
            PlayerEvent::EofReached(false),
            PlayerEvent::EofReached(true),
            PlayerEvent::EofReached(true),
        ],
        ..Default::default()
    });
    runner
        .control(Command::Headless(Box::new(selection())))
        .await;
    assert_eq!(
        runner.model.status,
        Status::Preparing(Stage::ResolvingSource)
    );
    resolve(&mut runner).await;
    assert_eq!(runner.model.status, Status::Ended);
    runner.control(Command::Seek(0.0)).await;
    runner.control(Command::SetPaused(false)).await;
    assert_eq!(runner.model.status, Status::Ready);
    assert!(!runner.model.paused);
    assert!(lock(&backend.log).ends_with(&["seek:0".into(), "pause:false".into()]));
    runner.control(Command::Stop).await;
    assert_eq!(
        *lock(&progress.calls),
        [
            "begin:0",
            "sample:1",
            "sample:12",
            "finish",
            "seek:0",
            "sample:0",
            "stop"
        ]
    );
}
#[tokio::test]
async fn error_then_retry_resumes_and_preserves_latch() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).push_back(Script {
        events: vec![
            PlayerEvent::Duration(Some(600.0)),
            PlayerEvent::TimePos(Some(42.0)),
            PlayerEvent::Error(panorama_mpv::MpvError::Loading),
            PlayerEvent::TimePos(None),
        ],
        ..Default::default()
    });
    runner
        .control(Command::Headless(Box::new(selection())))
        .await;
    resolve(&mut runner).await;
    assert_eq!(
        runner.model.status,
        Status::Error(PlaybackError::SourceUnavailable)
    );
    runner.control(Command::Retry).await;
    assert!(!runner.model.initial_preparing());
    resolve(&mut runner).await;
    assert_eq!(runner.model.status, Status::Ready);
    assert_eq!(
        lock(&backend.log)
            .iter()
            .filter(|entry| entry.as_str() == "seek:42")
            .count(),
        1
    );
}
#[tokio::test]
async fn initial_error_and_resolver_failure_are_retryable() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).push_back(Script {
        error: Some(PlaybackError::LibraryMissing),
        ..Default::default()
    });
    runner
        .control(Command::Headless(Box::new(selection())))
        .await;
    resolve(&mut runner).await;
    assert_eq!(
        runner.model.status,
        Status::Error(PlaybackError::LibraryMissing)
    );
    runner.control(Command::Retry).await;
    resolve(&mut runner).await;
    assert_eq!(runner.model.status, Status::Ready);
}
#[tokio::test]
async fn switch_mid_resolve_drops_stale_result() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).extend([
        Script {
            delay: Duration::from_millis(20),
            ignore_cancel: true,
            ..Default::default()
        },
        Script::default(),
    ]);
    runner
        .control(Command::Headless(Box::new(selection())))
        .await;
    assert!(futures::poll!(runner.pending.next()).is_pending());
    runner
        .control(Command::SwitchSource(SourceRef {
            group: 0,
            stream: 1,
        }))
        .await;
    resolve(&mut runner).await;
    assert_eq!(runner.model.source.stream, 1);
    let (generation, result) = runner.pending.next().await.unwrap();
    runner.opened(generation, result).await;
    assert_eq!(runner.model.status, Status::Ready);
    assert_eq!(
        *lock(&backend.log),
        [
            "load",
            "seek:0",
            "mpv stop",
            "session close",
            "player close"
        ]
    );
}
#[tokio::test]
async fn stop_closes_mpv_session_player_in_order() {
    let (mut runner, backend, _) = setup();
    runner
        .control(Command::Headless(Box::new(selection())))
        .await;
    resolve(&mut runner).await;
    runner.control(Command::Stop).await;
    assert_eq!(
        *lock(&backend.log),
        ["load", "mpv stop", "session close", "player close"]
    );
    assert_eq!(runner.model.status, Status::Idle);
}
#[tokio::test]
async fn resume_seeks_once_after_duration() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).push_back(Script {
        events: vec![
            PlayerEvent::Duration(None),
            PlayerEvent::Duration(Some(600.0)),
            PlayerEvent::Duration(Some(600.0)),
            PlayerEvent::TimePos(Some(90.0)),
        ],
        ..Default::default()
    });
    let mut selection = selection();
    selection.start = StartMode::Resume(90.0);
    runner.control(Command::Headless(Box::new(selection))).await;
    resolve(&mut runner).await;
    assert_eq!(*lock(&backend.log), ["load", "seek:90"]);
}
#[tokio::test]
async fn controls_report_seek_and_preserve_audio_state() {
    let (mut runner, backend, progress) = setup();
    runner
        .control(Command::Headless(Box::new(selection())))
        .await;
    resolve(&mut runner).await;
    for command in [
        Command::Seek(30.0),
        Command::SeekRelative(-10.0),
        Command::TogglePause,
        Command::SetVolume(255),
        Command::SetMuted(true),
        Command::SelectAudio(2),
        Command::SelectSubtitle(None),
        Command::SelectSubtitle(Some(3)),
        Command::Seek(f64::NAN),
    ] {
        runner.control(command).await;
    }
    assert_eq!(
        (runner.model.volume, runner.model.muted, runner.model.paused),
        (100, true, true)
    );
    assert!(
        lock(&progress.calls)
            .windows(2)
            .any(|pair| pair == ["seek:30", "seek:20"])
    );
    assert!(lock(&backend.log).contains(&"audio:2".into()));
    runner.control(Command::Retry).await;
    resolve(&mut runner).await;
    assert_eq!((runner.model.volume, runner.model.muted), (100, true));
}
#[tokio::test]
async fn channel_host_bridge_runs_without_gpui() {
    let backend = Arc::new(FakeBackend::new(Some(FixtureState::Ready)));
    let mut host = PlaybackHost::with_backend(
        &tokio::runtime::Handle::current(),
        backend,
        Arc::new(RecordingProgress::default()),
    );
    let mut events = host.events().unwrap();
    host.send(Command::Headless(Box::new(selection())));
    tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(event) = events.next().await {
            if matches!(event, HostEvent::Mpv(PlayerEvent::TimePos(Some(_)))) {
                break;
            }
        }
    })
    .await
    .unwrap();
    host.send(Command::Stop);
}

#[path = "host_regression_tests.rs"]
mod regression;
