use super::*;
use crate::addons::Kind;

mod clock;
mod drift;
mod host;
mod member;
mod protocol;
mod release;

const SECRET: &str = "0123456789abcdef0123456789abcdef";

fn secret() -> transport::PartySecret {
    transport::PartySecret::from_hex(SECRET).unwrap()
}

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

#[test]
fn the_secret_never_prints() {
    let host = HostSession::new("Alex".into(), secret(), HostSettings::default());
    let member = MemberSession::new("Sam".into(), secret());
    let hello = Up::Hello {
        name: "Sam".into(),
        version: PROTOCOL_VERSION,
        secret: secret(),
    };
    for printed in [
        format!("{host:?}"),
        format!("{member:?}"),
        format!("{hello:?}"),
    ] {
        assert!(!printed.contains(SECRET), "{printed}");
    }
}
