use super::*;
use crate::playback::fake::stats;
use panorama_core::torrent::{TorrentError, TorrentState};

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

fn torrent_selection() -> PlaybackSelection {
    let mut selection = selection();
    if let panorama_core::addons::StreamState::Ready(streams) = &mut selection.groups[0].state {
        streams[0].stream = serde_json::from_value(serde_json::json!({
            "infoHash": HASH,
            "sources": ["tracker:udp://tracker.invalid:6969/secret-path"],
        }))
        .expect("torrent stream");
    }
    selection
}

#[tokio::test]
async fn a_torrent_source_starts_in_finding_peers() {
    let (mut runner, _, _) = setup();
    runner
        .control(Command::Headless(Box::new(torrent_selection())))
        .await;
    assert!(runner.model.finding_peers());
    assert_eq!(runner.model.torrent, Some(TorrentHealth::connecting()));
    runner
        .control(Command::Headless(Box::new(selection())))
        .await;
    assert_eq!(
        runner.model.torrent, None,
        "an HTTP source is not a torrent"
    );
}

#[tokio::test]
async fn no_peers_then_keep_waiting_returns_to_finding_peers_and_plays() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).extend([
        Script {
            error: Some(PlaybackError::NoPeers),
            ..Default::default()
        },
        Script::default(),
    ]);
    let mut selection = torrent_selection();
    selection.start = StartMode::Resume(30.0);
    runner.control(Command::Headless(Box::new(selection))).await;
    resolve(&mut runner).await;
    assert_eq!(runner.model.status, Status::NoPeers);
    runner.control(Command::RetryTorrent).await;
    assert_eq!(
        runner.model.status,
        Status::Preparing(Stage::ResolvingSource)
    );
    assert!(runner.model.finding_peers());
    resolve(&mut runner).await;
    assert_eq!(runner.model.status, Status::Ready);
    assert_eq!(
        lock(&backend.log)
            .iter()
            .filter(|entry| entry.as_str() == "seek:30")
            .count(),
        1,
        "the retry keeps the resume position"
    );
}

#[tokio::test]
async fn keep_waiting_closes_the_stream_it_replaces() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).push_back(Script {
        torrent: Some(stats(TorrentState::Streaming, 2, 1_000_000)),
        ..Default::default()
    });
    runner
        .control(Command::Headless(Box::new(torrent_selection())))
        .await;
    resolve(&mut runner).await;
    runner.emit(HostEvent::Failed(PlaybackError::NoPeers));
    lock(&backend.log).clear();
    runner.control(Command::RetryTorrent).await;
    assert_eq!(
        *lock(&backend.log),
        ["mpv stop", "session close", "player close"],
        "the lease is released first, so the engine can clear its latched error"
    );
}

#[tokio::test]
async fn keep_waiting_does_nothing_unless_there_are_no_peers() {
    let (mut runner, backend, _) = setup();
    runner
        .control(Command::Headless(Box::new(torrent_selection())))
        .await;
    resolve(&mut runner).await;
    assert_eq!(runner.model.status, Status::Ready);
    lock(&backend.log).clear();
    runner.control(Command::RetryTorrent).await;
    assert_eq!(runner.model.status, Status::Ready);
    assert!(lock(&backend.log).is_empty());
}

fn with_error(error: TorrentError) -> Script {
    let mut stalled = stats(TorrentState::Stalled, 0, 0);
    stalled.error = Some(error);
    Script {
        torrent: Some(stalled),
        ..Default::default()
    }
}

#[tokio::test]
async fn the_stats_poll_feeds_health_and_the_status_stays_with_mpv() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).push_back(Script {
        torrent: Some(stats(TorrentState::Streaming, 3, 2_000_000)),
        ..Default::default()
    });
    runner
        .control(Command::Headless(Box::new(torrent_selection())))
        .await;
    resolve(&mut runner).await;
    runner.poll_stats();
    assert_eq!(runner.model.status, Status::Ready);
    let health = runner.model.torrent.expect("health");
    assert_eq!((health.peers_live, health.download_bps), (3, 2_000_000));
}

#[tokio::test]
async fn the_engine_no_peers_error_shows_no_peers_and_pauses_mpv() {
    let (mut runner, backend, _) = setup();
    lock(&backend.scripts).push_back(with_error(TorrentError::NoPeers));
    runner
        .control(Command::Headless(Box::new(torrent_selection())))
        .await;
    resolve(&mut runner).await;
    runner.poll_stats();
    assert_eq!(runner.model.status, Status::NoPeers);
    assert!(runner.model.paused);
    assert_eq!(
        lock(&backend.log).last().map(String::as_str),
        Some("pause:true")
    );
    let before = lock(&backend.log).len();
    runner.poll_stats();
    assert_eq!(lock(&backend.log).len(), before, "NoPeers is entered once");
}

#[tokio::test]
async fn a_disk_error_is_the_cache_error_not_no_peers() {
    use panorama_core::torrent::DiskError;
    for disk in [DiskError::Full, DiskError::Io] {
        let (mut runner, backend, _) = setup();
        lock(&backend.scripts).push_back(with_error(TorrentError::Disk(disk)));
        runner
            .control(Command::Headless(Box::new(torrent_selection())))
            .await;
        resolve(&mut runner).await;
        runner.poll_stats();
        assert_eq!(
            runner.model.status,
            Status::Error(PlaybackError::TorrentCache)
        );
        runner.poll_stats();
        assert_eq!(
            runner.model.status,
            Status::Error(PlaybackError::TorrentCache)
        );
    }
}

const FORBIDDEN: [&str; 6] = [
    HASH,
    "ABCDEF",
    "magnet",
    "tracker.invalid",
    "secret-path",
    "btih",
];

fn assert_clean(text: &str) {
    let text = text.to_lowercase();
    for forbidden in FORBIDDEN {
        assert!(
            !text.contains(&forbidden.to_lowercase()),
            "{forbidden} leaked"
        );
    }
}

#[tokio::test]
async fn keep_waiting_leaks_no_hash_magnet_or_tracker() {
    let backend = Arc::new(FakeBackend::new(None));
    lock(&backend.scripts).extend([
        with_error(TorrentError::NoPeers),
        Script {
            torrent: Some(stats(TorrentState::Streaming, 3, 2_000_000)),
            ..Default::default()
        },
    ]);
    let (events, mut received) = mpsc::unbounded();
    let mut runner = Runner::new(
        backend.clone(),
        Arc::new(RecordingProgress::default()),
        events,
    );
    runner
        .control(Command::Headless(Box::new(torrent_selection())))
        .await;
    resolve(&mut runner).await;
    runner.poll_stats();
    assert_eq!(runner.model.status, Status::NoPeers);
    runner.control(Command::RetryTorrent).await;
    resolve(&mut runner).await;
    runner.poll_stats();
    assert_eq!(runner.model.status, Status::Ready);
    let mut text = format!("{:?} {:?}", runner.model.torrent, torrent_selection());
    while let Ok(event) = received.try_recv() {
        text.push_str(&format!("{event:?}"));
    }
    text.push_str(&lock(&backend.log).join(" "));
    assert_clean(&text);
}

#[test]
fn debug_output_and_error_text_carry_no_hash_or_magnet() {
    use panorama_core::torrent::{DiskError, TorrentSource};
    let sources = vec![
        format!("dht:{HASH}"),
        "tracker:udp://tracker.invalid:6969/secret-path".to_owned(),
    ];
    let source = TorrentSource::from_stream(HASH, Some(2), &sources).expect("source");
    let mut text = format!("{source:?}");
    let mut reading = stats(TorrentState::Stalled, 1, 5);
    reading.error = Some(TorrentError::Disk(DiskError::Full));
    text.push_str(&format!("{reading:?}"));
    text.push_str(&format!("{:?}", torrent_selection()));
    let stream = &torrent_selection().groups[0];
    text.push_str(&format!("{stream:?}"));
    for error in [
        PlaybackError::LibraryMissing,
        PlaybackError::SourceUnavailable,
        PlaybackError::SourceExpired,
        PlaybackError::NoPeers,
        PlaybackError::NoVideoFile,
        PlaybackError::TorrentCache,
        PlaybackError::Network,
        PlaybackError::Decode,
        PlaybackError::Unknown,
    ] {
        text.push_str(error.message());
        text.push_str(&format!("{error:?} {error}"));
    }
    for error in [
        TorrentError::NoPeers,
        TorrentError::MetadataTimeout,
        TorrentError::InvalidSource,
        TorrentError::Disk(DiskError::Io),
    ] {
        text.push_str(&format!("{error:?} {error}"));
    }
    assert_clean(&text);
}
