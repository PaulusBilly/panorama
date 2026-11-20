use super::*;
use crate::addons::Kind;

mod clock;
mod drift;
mod host;
mod member;
mod protocol;

const SECRET: &str = "0123456789abcdef0123456789abcdef";

fn media() -> PartyMedia {
    PartyMedia {
        film_id: "tt0111161".into(),
        kind: Kind::Movie,
        video_id: "tt0111161".into(),
        title: "The Film".into(),
        release: PartyRelease {
            addon: "com.stremio.torrentio.addon".into(),
            torrent: Some(TorrentRef {
                info_hash: "a".repeat(40),
                file_idx: Some(1),
            }),
            label: "Torrentio\nThe.Film.1080p.WEB-DL".into(),
            binge_group: Some("torrentio|1080p".into()),
        },
    }
}
