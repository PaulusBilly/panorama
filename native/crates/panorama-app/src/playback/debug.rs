use super::selection::{PlaybackSelection, SourceRef, StartMode};
use crate::args::Args;
use panorama_core::addons::{StreamState, StreamsEvent};

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
    let StreamsEvent::Groups(mut groups) = crate::fixtures::streams(false) else {
        return None;
    };
    if let StreamState::Ready(streams) = &mut groups.first_mut()?.state {
        let value = if args.play_torrent {
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
    })
}
