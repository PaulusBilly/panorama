//! The torrent half of the playback reducer.
use super::*;
use panorama_core::torrent::TorrentState;

impl PlaybackModel {
    /// Record one reading of the engine's stats. The engine reports `Stalled` only together with
    /// an error, and the host sends errors as `Failed`, so a reading never changes the status;
    /// Buffering comes from mpv. `NoPeers` is entered only by `Failed(NoPeers)`, stays until the
    /// host restarts the source (the engine latches that error too), and ignores later readings.
    pub(super) fn torrent_health(&mut self, health: TorrentHealth) {
        if matches!(
            self.status,
            Status::Idle | Status::NoPeers | Status::Error(_) | Status::Ended
        ) {
            return;
        }
        self.torrent = Some(health);
    }

    /// A torrent still looking for peers or metadata: the player reads "Finding peers".
    pub fn finding_peers(&self) -> bool {
        let Some(health) = self.torrent else {
            return false;
        };
        match self.status {
            Status::Preparing(Stage::ResolvingSource) => true,
            Status::Preparing(Stage::LoadingVideo) => matches!(
                health.state,
                TorrentState::Connecting | TorrentState::Metadata
            ),
            _ => false,
        }
    }

    /// The second overlay line for a torrent: "<n> peers · <rate>/s". "Finding peers" has none.
    pub fn torrent_caption(&self) -> Option<String> {
        if self.finding_peers() {
            return None;
        }
        self.torrent.and_then(|health| health.caption())
    }
}
