use super::*;
use panorama_core::torrent::TorrentState;

fn health(state: TorrentState, live: u32) -> HostEvent {
    HostEvent::Torrent(TorrentHealth {
        state,
        peers_live: live,
        peers_connecting: 0,
        download_bps: 1_000_000,
        since_ms: 0,
    })
}

fn torrent() -> PlaybackModel {
    let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
    model.apply(HostEvent::Resolving);
    model.apply(HostEvent::Torrent(TorrentHealth::connecting()));
    model
}

fn playing() -> PlaybackModel {
    let mut model = torrent();
    model.apply(HostEvent::Loading);
    model.apply(health(TorrentState::Streaming, 4));
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(1.0))));
    model
}

#[test]
fn connecting_to_metadata_to_streaming_to_ready() {
    let mut model = torrent();
    assert!(model.finding_peers());
    model.apply(HostEvent::Loading);
    model.apply(health(TorrentState::Connecting, 0));
    assert!(model.finding_peers());
    model.apply(health(TorrentState::Metadata, 2));
    assert!(model.finding_peers());
    assert_eq!(
        model.torrent_caption(),
        None,
        "finding peers has no second line"
    );
    model.apply(health(TorrentState::Streaming, 5));
    assert!(!model.finding_peers());
    assert_eq!(model.status, Status::Preparing(Stage::LoadingVideo));
    assert_eq!(
        model.torrent_caption().as_deref(),
        Some("5 peers · 1.0 MB/s")
    );
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(0.5))));
    assert_eq!(model.status, Status::Ready);
}

#[test]
fn a_source_that_is_not_a_torrent_never_finds_peers() {
    let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
    model.apply(HostEvent::Resolving);
    assert!(!model.finding_peers());
    assert_eq!(model.torrent, None);
}

#[test]
fn streaming_then_the_engine_no_peers_error_shows_no_peers() {
    let mut model = playing();
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
    assert_eq!(model.status, Status::Buffering);
    assert_eq!(
        model.torrent_caption().as_deref(),
        Some("4 peers · 1.0 MB/s")
    );
    model.apply(HostEvent::Failed(PlaybackError::NoPeers));
    assert_eq!(model.status, Status::NoPeers);
    assert!(model.surface_visible());
}

#[test]
fn readings_never_change_the_status() {
    let mut model = playing();
    model.apply(health(TorrentState::Streaming, 0));
    assert_eq!(model.status, Status::Ready);
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(true)));
    model.apply(health(TorrentState::Streaming, 3));
    assert_eq!(model.status, Status::Buffering);
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(false)));
    assert_eq!(model.status, Status::Ready);
}

#[test]
fn no_peers_while_opening_hides_the_surface() {
    let mut model = torrent();
    model.apply(HostEvent::Failed(PlaybackError::NoPeers));
    assert_eq!(model.status, Status::NoPeers);
    assert!(!model.surface_visible());
}

#[test]
fn no_peers_is_not_overwritten_by_mpv_or_later_stats() {
    let mut model = playing();
    model.apply(HostEvent::Failed(PlaybackError::NoPeers));
    model.apply(HostEvent::Mpv(PlayerEvent::Error(
        panorama_mpv::MpvError::Loading,
    )));
    model.apply(HostEvent::Mpv(PlayerEvent::PausedForCache(false)));
    model.apply(health(TorrentState::Streaming, 6));
    assert_eq!(model.status, Status::NoPeers);
}

#[test]
fn no_peers_retry_returns_to_connecting() {
    let mut model = playing();
    model.apply(HostEvent::Failed(PlaybackError::NoPeers));
    model.apply(HostEvent::Resolving);
    assert_eq!(model.status, Status::Preparing(Stage::ResolvingSource));
    assert_eq!(model.torrent, None);
    model.apply(HostEvent::Torrent(TorrentHealth::connecting()));
    assert!(model.finding_peers());
    model.apply(HostEvent::Loading);
    model.apply(health(TorrentState::Connecting, 0));
    assert_eq!(model.status, Status::Preparing(Stage::LoadingVideo));
    assert!(model.finding_peers());
}

#[test]
fn a_disk_failure_is_an_error_not_no_peers() {
    for mut model in [torrent(), playing()] {
        model.apply(HostEvent::Failed(PlaybackError::TorrentCache));
        assert_eq!(model.status, Status::Error(PlaybackError::TorrentCache));
        model.apply(health(TorrentState::Streaming, 0));
        assert_eq!(model.status, Status::Error(PlaybackError::TorrentCache));
    }
    let message = PlaybackError::TorrentCache.message();
    assert!(!message.contains("connection"));
}

#[test]
fn other_failures_stay_generic_errors() {
    let mut model = torrent();
    model.apply(HostEvent::Failed(PlaybackError::NoVideoFile));
    assert_eq!(model.status, Status::Error(PlaybackError::NoVideoFile));
}
