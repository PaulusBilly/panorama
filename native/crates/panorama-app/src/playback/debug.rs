use super::selection::{PlaybackSelection, SourceRef, StartMode};
use crate::args::Args;
use panorama_core::addons::StreamState;

pub fn selection(id: &str, args: &Args) -> Option<PlaybackSelection> {
    if !args.fixtures {
        return None;
    }
    let mut film =
        crate::fixtures::details(id).or_else(|| crate::fixtures::films().into_iter().next())?;
    film.id = id.to_owned();
    if args.play_file.is_some() || args.play_torrent {
        film.name = "Big Buck Bunny".into();
    }
    let mut groups = crate::fixtures::player_groups();
    if let StreamState::Ready(streams) = &mut groups.first_mut()?.state {
        let fixture_torrent = args.player_state.is_some_and(|state| state.is_torrent());
        let value = if fixture_torrent {
            // Not a real torrent: the fixture backend never contacts anyone.
            serde_json::json!({"infoHash": "0123456789abcdef0123456789abcdef01234567"})
        } else if let Some(hash) = args.torrent_hash.as_ref().filter(|_| args.play_torrent) {
            serde_json::json!({ "infoHash": hash })
        } else if args.play_torrent {
            serde_json::json!({
                "infoHash": "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
                "sources": ["dht:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
                    "tracker:udp://explodie.org:6969", "tracker:udp://tracker.opentrackr.org:1337",
                    "tracker:udp://tracker.openbittorrent.com:80"]
            })
        } else if let Some(url) = &args.play_url {
            serde_json::json!({"url": url})
        } else {
            serde_json::json!({"url": "https://fixture.invalid/aftersun.mp4"})
        };
        streams.first_mut()?.stream = serde_json::from_value(value).ok()?;
    }
    Some(PlaybackSelection {
        film,
        groups,
        source: SourceRef::default(),
        start: StartMode::Restart,
        meta_base: None,
        subtitles: vec![],
        late_subtitles: None,
    })
}
