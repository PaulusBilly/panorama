use super::*;
use crate::playback::{
    launch::Pick,
    model::{PlaybackError, SourceRef, Stage},
};
use panorama_core::addons::{AddonKey, StreamState};
use serde_json::{Value, json};

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

fn model(status: Status, paused: bool, time: Option<f64>) -> PlaybackModel {
    let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
    model.status = status;
    model.paused = paused;
    model.time = time;
    model.duration = Some(6_000.0);
    model
}

fn source(stream: Value, title: &str) -> StreamSource {
    StreamSource {
        stream: serde_json::from_value(stream).unwrap(),
        name: Some("Addon".into()),
        title: Some(title.into()),
        description: None,
    }
}

fn group(key: &str, streams: Vec<StreamSource>) -> StreamGroup {
    StreamGroup {
        addon: serde_json::from_value::<AddonKey>(key.into()).unwrap(),
        name: key.into(),
        state: StreamState::Ready(streams),
    }
}

fn link(title: &str) -> StreamSource {
    source(json!({ "url": "https://cdn.test/film.mkv" }), title)
}

fn release_of(group: &StreamGroup) -> PartyRelease {
    let StreamState::Ready(streams) = &group.state else {
        unreachable!()
    };
    PartyRelease::of(group, &streams[0])
}

#[test]
fn the_player_reports_what_it_is_doing() {
    let report = |status, paused| member_state(&model(status, paused, Some(42.5))).unwrap().0;
    assert_eq!(member_state(&model(Status::Idle, false, None)), None);
    assert_eq!(
        member_state(&model(Status::Preparing(Stage::LoadingVideo), false, None)),
        Some((MemberState::Loading, 0.0, Some(6_000.0)))
    );
    assert_eq!(report(Status::Ready, false), MemberState::Buffering);
    let mut flowing = model(Status::Ready, false, Some(10.0));
    flowing.apply(crate::playback::model::HostEvent::Mpv(
        panorama_mpv::PlayerEvent::TimePos(Some(10.5)),
    ));
    assert_eq!(member_state(&flowing).unwrap().0, MemberState::Playing);
    assert_eq!(report(Status::Ready, true), MemberState::Paused);
    let mut starved = model(Status::Ready, true, Some(42.5));
    starved.cache_seconds = Some(0.4);
    assert_eq!(member_state(&starved).unwrap().0, MemberState::Buffering);
    starved.cache_seconds = Some(8.0);
    assert_eq!(member_state(&starved).unwrap().0, MemberState::Paused);
    let mut at_end = model(Status::Ready, true, Some(5_999.0));
    at_end.cache_seconds = Some(0.5);
    assert_eq!(member_state(&at_end).unwrap().0, MemberState::Paused);
    assert_eq!(report(Status::Buffering, false), MemberState::Buffering);
    assert_eq!(report(Status::Ended, false), MemberState::Paused);
    assert_eq!(report(Status::NoPeers, false), MemberState::Failed);
    assert_eq!(
        report(Status::Error(PlaybackError::Network), false),
        MemberState::Failed
    );
    assert_eq!(
        member_state(&model(Status::Ready, false, Some(f64::NAN))).map(|r| r.1),
        Some(0.0)
    );
}

#[test]
fn user_commands_become_absolute_actions() {
    let playing = model(Status::Ready, false, Some(100.0));
    let paused = model(Status::Ready, true, Some(100.0));
    assert_eq!(
        user_action(&Command::TogglePause, &playing),
        Some(Action::Pause)
    );
    assert_eq!(
        user_action(&Command::TogglePause, &paused),
        Some(Action::Play)
    );
    assert_eq!(
        user_action(&Command::SetPaused(false), &paused),
        Some(Action::Play)
    );
    assert_eq!(
        user_action(&Command::SeekRelative(-10.0), &playing),
        Some(Action::Seek { to: 90.0 })
    );
    assert_eq!(
        user_action(&Command::SeekRelative(-500.0), &playing),
        Some(Action::Seek { to: 0.0 })
    );
    assert_eq!(
        user_action(&Command::Seek(9_000.0), &playing),
        Some(Action::Seek { to: 6_000.0 })
    );
    assert_eq!(user_action(&Command::Seek(f64::NAN), &playing), None);
    assert_eq!(user_action(&Command::SetVolume(40), &playing), None);
    assert_eq!(user_action(&Command::SetSpeed(1.05), &playing), None);
}

#[test]
fn a_party_launch_holds_out_for_the_release_then_falls_back() {
    let host = group("com.torbox#host", vec![link("Film 1080p TorBox")]);
    let pick = Pick::Party(release_of(&host));
    let first = vec![group(
        "com.other#a",
        vec![source(json!({ "infoHash": HASH, "fileIdx": 0 }), "Other")],
    )];
    assert_eq!(pick.early(&first, None), None);
    assert_eq!(
        pick.last(&first, None),
        Some(SourceRef {
            group: 0,
            stream: 0
        })
    );
    let mut later = first.clone();
    later.push(group(
        "com.torbox#member",
        vec![link("Film 2160p"), link("Film 1080p TorBox")],
    ));
    assert_eq!(
        pick.early(&later, None),
        Some(SourceRef {
            group: 1,
            stream: 1
        })
    );
}

#[test]
fn a_member_on_the_hosts_release_matches() {
    let host = group(
        "com.torbox#host",
        vec![source(
            json!({ "url": "https://a.test/1", "behaviorHints": { "bingeGroup": "tb|1080p" } }),
            "Film 1080p",
        )],
    );
    let release = release_of(&host);
    let other_addon = group("com.other#m", vec![link("Film 1080p")]);
    let same = group("com.torbox#m", vec![link("Film 1080p")]);
    let binge = group(
        "com.torbox#m",
        vec![source(
            json!({ "url": "https://b.test/2", "behaviorHints": { "bingeGroup": "tb|1080p" } }),
            "Film 1080p REPACK",
        )],
    );
    let different = group("com.torbox#m", vec![link("Film 720p")]);
    let first = |group: &StreamGroup| match &group.state {
        StreamState::Ready(streams) => streams[0].clone(),
        _ => unreachable!(),
    };
    assert!(release_matches(&release, &same, &first(&same)));
    assert!(release_matches(&release, &binge, &first(&binge)));
    assert!(!release_matches(
        &release,
        &other_addon,
        &first(&other_addon)
    ));
    assert!(!release_matches(&release, &different, &first(&different)));

    let torrent = group(
        "com.torrentio#h",
        vec![source(json!({ "infoHash": HASH, "fileIdx": 2 }), "T")],
    );
    let elsewhere = group(
        "com.other#m",
        vec![source(json!({ "infoHash": HASH, "fileIdx": 2 }), "Other")],
    );
    assert!(release_matches(
        &release_of(&torrent),
        &elsewhere,
        &first(&elsewhere)
    ));
}

#[test]
fn only_a_member_in_a_live_party_skips_up_next() {
    let host = PartyView::new(
        Role::Host {
            invite: None,
            settings: HostSettings::default(),
        },
        Link::Live,
    );
    let mut member = PartyView::new(Role::Member, Link::Live);
    assert!(up_next_allowed(None));
    assert!(up_next_allowed(Some(&host)));
    assert!(!up_next_allowed(Some(&member)));
    member.link = Link::Ended(EndReason::Bye(ByeReason::HostLeft));
    assert!(up_next_allowed(Some(&member)));
}

#[test]
fn display_names_fit_the_protocol() {
    assert_eq!(display_name("  billy \n").as_deref(), Some("billy"));
    assert_eq!(display_name(" \t "), None);
    assert_eq!(display_name(&"x".repeat(40)).map(|n| n.len()), Some(32));
    assert_eq!(display_name("a\u{7}b").as_deref(), Some("ab"));
    assert_eq!(
        display_name("Bi\u{202E}ll\u{200B}y").as_deref(),
        Some("Billy")
    );
}

#[test]
fn the_invite_never_prints() {
    let view = PartyView::new(
        Role::Host {
            invite: Some(InviteLink("panorama://party/abc?s=secret".into())),
            settings: HostSettings::default(),
        },
        Link::Live,
    );
    let printed = format!("{view:?}");
    assert!(!printed.contains("secret") && !printed.contains("abc"));
}

#[test]
fn a_connection_may_burst_then_keeps_to_its_rate() {
    let start = Instant::now();
    let mut limit = service::RateLimit::new(start);
    assert_eq!((0..100).filter(|_| limit.allow(start)).count(), 60);
    let later = start + std::time::Duration::from_secs(1);
    assert_eq!((0..100).filter(|_| limit.allow(later)).count(), 30);
    let much_later = later + std::time::Duration::from_secs(60);
    assert_eq!((0..100).filter(|_| limit.allow(much_later)).count(), 60);
}

#[test]
fn a_peer_that_stops_reading_overflows_its_queue() {
    let (queue, _unread) = service::Queue::new();
    for _ in 0..256 {
        assert!(queue.line("{}".into()).is_ok());
    }
    assert!(queue.line("{}".into()).is_err());
}
