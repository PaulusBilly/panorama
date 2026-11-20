use serde_json::{Value, json};

use super::*;
use crate::addons::{AddonKey, StreamGroup, StreamSource, StreamState};

const HASH: &str = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";

fn source(stream: Value, title: &str) -> StreamSource {
    StreamSource {
        stream: serde_json::from_value(stream).unwrap(),
        name: Some("Addon".into()),
        title: Some(title.into()),
        description: None,
    }
}

fn torrent(hash: &str, file_idx: u64, title: &str) -> StreamSource {
    source(json!({ "infoHash": hash, "fileIdx": file_idx }), title)
}

fn link(url: &str, title: &str, binge_group: Option<&str>) -> StreamSource {
    source(
        json!({ "url": url, "behaviorHints": { "bingeGroup": binge_group } }),
        title,
    )
}

fn group(key: &str, streams: Vec<StreamSource>) -> StreamGroup {
    StreamGroup {
        addon: serde_json::from_value::<AddonKey>(key.into()).unwrap(),
        name: key.into(),
        state: StreamState::Ready(streams),
    }
}

#[test]
fn of_reads_manifest_torrent_label_and_binge_group() {
    let host = group(
        "com.stremio.torrentio.addon#1a2b",
        vec![source(
            json!({
                "infoHash": HASH.to_uppercase(),
                "fileIdx": 3,
                "behaviorHints": { "bingeGroup": "torrentio|1080p" },
            }),
            "Film 1080p",
        )],
    );
    let StreamState::Ready(streams) = &host.state else {
        unreachable!()
    };
    let release = PartyRelease::of(&host, &streams[0]);
    assert_eq!(release.addon, "com.stremio.torrentio.addon");
    assert_eq!(
        release.torrent,
        Some(TorrentRef {
            info_hash: HASH.into(),
            file_idx: Some(3),
        })
    );
    assert_eq!(release.label, "Addon\nFilm 1080p");
    assert_eq!(release.binge_group.as_deref(), Some("torrentio|1080p"));
}

#[test]
fn salted_addon_suffix_is_ignored() {
    let host = group(
        "org.debrid#aaaa",
        vec![link("https://debrid.test/a?token=1", "Film", None)],
    );
    let StreamState::Ready(streams) = &host.state else {
        unreachable!()
    };
    let release = PartyRelease::of(&host, &streams[0]);
    let member = vec![group(
        "org.debrid#ffff",
        vec![link("https://debrid.test/a?token=1", "Film", None)],
    )];
    assert_eq!(release.addon, "org.debrid");
    assert_eq!(find(&release, &member), Some((0, 0, true)));
}

#[test]
fn torrent_matches_from_any_addon() {
    let host = group("com.torrentio#1", vec![torrent(HASH, 1, "Torrentio label")]);
    let StreamState::Ready(streams) = &host.state else {
        unreachable!()
    };
    let release = PartyRelease::of(&host, &streams[0]);
    let member = vec![
        group("com.torrentio#2", vec![torrent(HASH, 2, "Torrentio label")]),
        group("com.other#3", vec![torrent(HASH, 1, "Another label")]),
    ];
    assert_eq!(find(&release, &member), Some((1, 0, true)));
}

#[test]
fn debrid_matches_by_manifest_and_label_when_url_changed() {
    let host = group(
        "org.debrid#1",
        vec![link("https://debrid.test/x?token=old", "Film 2160p", None)],
    );
    let StreamState::Ready(streams) = &host.state else {
        unreachable!()
    };
    let release = PartyRelease::of(&host, &streams[0]);
    let member = vec![
        group(
            "org.other#1",
            vec![link("https://other.test/y", "Film 2160p", None)],
        ),
        group(
            "org.debrid#2",
            vec![
                link("https://debrid.test/x?token=a", "Film 1080p", None),
                link("https://debrid.test/x?token=new", "Film 2160p", None),
            ],
        ),
    ];
    assert_eq!(find(&release, &member), Some((1, 1, true)));
}

#[test]
fn binge_group_is_inexact_fallback() {
    let release = PartyRelease {
        addon: "org.debrid".into(),
        torrent: None,
        label: "Addon\nS01E01 1080p".into(),
        binge_group: Some("debrid|1080p".into()),
    };
    let member = vec![group(
        "org.debrid#2",
        vec![
            link("https://debrid.test/1", "S01E02 720p", Some("debrid|720p")),
            link(
                "https://debrid.test/2",
                "S01E02 1080p",
                Some("debrid|1080p"),
            ),
        ],
    )];
    assert_eq!(find(&release, &member), Some((0, 1, false)));
}

#[test]
fn exact_match_beats_binge_group() {
    let release = PartyRelease {
        addon: "org.debrid".into(),
        torrent: None,
        label: "Addon\nFilm".into(),
        binge_group: Some("debrid|1080p".into()),
    };
    let member = vec![group(
        "org.debrid#2",
        vec![
            link("https://debrid.test/1", "Other", Some("debrid|1080p")),
            link("https://debrid.test/2", "Film", None),
        ],
    )];
    assert_eq!(find(&release, &member), Some((0, 1, true)));
}

#[test]
fn no_match_returns_none() {
    let release = PartyRelease {
        addon: "org.debrid".into(),
        torrent: Some(TorrentRef {
            info_hash: HASH.into(),
            file_idx: None,
        }),
        label: "Addon\nFilm".into(),
        binge_group: Some("debrid|1080p".into()),
    };
    let member = vec![
        group(
            "org.other#1",
            vec![link("https://other.test/1", "Film", Some("debrid|1080p"))],
        ),
        group("org.debrid#2", vec![torrent(HASH, 0, "Other")]),
        StreamGroup {
            state: StreamState::Empty,
            ..group("org.debrid#3", Vec::new())
        },
    ];
    assert_eq!(find(&release, &member), None);
}

#[test]
fn debug_hides_label() {
    let mut leaky = media();
    leaky.release.label = "Torrentio\nhttps://debrid.test/secret-key/x.mkv".into();
    for text in [
        format!("{leaky:?}"),
        format!("{:?}", leaky.release),
        format!(
            "{:?}",
            Down::Load {
                media: leaky,
                revision: 1
            }
        ),
    ] {
        assert!(!text.contains("secret-key"), "{text}");
        assert!(!text.contains("Torrentio"), "{text}");
        assert!(!text.contains("torrentio|1080p"), "{text}");
    }
    let text = format!("{:?}", media());
    assert!(
        text.contains("tt0111161") && text.contains("torrent: true"),
        "{text}"
    );
}
