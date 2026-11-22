use super::*;
use crate::{
    party::follow::{Follow, Followed, choice_of, follow},
    playback::{
        launch::Pick,
        model::{PlaybackError, SourceRef, Stage, Track},
        source::{ResolvedSource, resolve},
    },
};
use panorama_core::{
    addons::{AddonKey, StreamState},
    party::{TorrentRef, TrackChoice},
};
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
fn a_seek_reads_as_buffering_until_the_picture_moves_on() {
    use crate::playback::model::HostEvent;
    use panorama_mpv::PlayerEvent;
    let mut model = model(Status::Ready, false, Some(10.0));
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(10.5))));
    model.apply(HostEvent::Seeked(200.0));
    // mpv keeps calling a stalled seek ready, frozen at its target.
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(200.0))));
    assert_eq!(member_state(&model).unwrap().0, MemberState::Buffering);
    model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(200.5))));
    assert_eq!(member_state(&model).unwrap().0, MemberState::Playing);
}

#[test]
fn a_paused_seek_is_not_ready_on_the_old_spots_cache() {
    use crate::playback::model::HostEvent;
    let mut model = model(Status::Ready, true, Some(10.0));
    model.cache_seconds = Some(14.0);
    model.apply(HostEvent::Seeked(200.0));
    assert_eq!(member_state(&model).unwrap().0, MemberState::Buffering);
    // A cache report still about the spot the seek left.
    model.cache_seconds = Some(9.8);
    model.buffered_until = Some(19.8);
    assert_eq!(member_state(&model).unwrap().0, MemberState::Buffering);
    model.buffered_until = Some(210.0);
    assert_eq!(member_state(&model).unwrap().0, MemberState::Paused);
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

fn track(id: i64, lang: &str, label: &str, external: bool, selected: bool) -> Track {
    Track {
        id,
        label: label.into(),
        description: None,
        lang: Some(lang.into()),
        codec: Some("subrip".into()),
        selected,
        external,
    }
}

/// What the host's party hears, from the host's own subtitle list.
fn hosts_choice(subtitles: &[Track]) -> TrackChoice {
    choice_of(&[track(1, "eng", "English", false, true)], subtitles).unwrap()
}

#[test]
fn a_friend_follows_the_hosts_subtitle_by_language() {
    let host = [
        track(2, "fra", "French", false, false),
        track(7, "eng", "OpenSubtitles", true, true),
    ];
    let choice = hosts_choice(&host);
    assert_eq!(
        choice
            .subtitle
            .as_ref()
            .and_then(|key| key.language.as_deref()),
        Some("eng")
    );
    // The friend's ids and order differ, and they have the embedded English, not the addon's.
    let friend = [
        track(1, "fra", "French", false, true),
        track(4, "jpn", "Japanese", false, false),
        track(9, "en", "English", false, false),
    ];
    assert_eq!(
        follow(PartySync::default(), &choice, &[], &friend),
        Follow {
            audio: None,
            subtitle: Some(Some(9))
        }
    );
    let own = [track(3, "fra", "French", false, true)];
    assert_eq!(
        follow(PartySync::default(), &choice, &[], &own).subtitle,
        None,
        "no match keeps the friend's own pick"
    );
}

#[test]
fn a_friend_follows_the_host_turning_subtitles_off() {
    let host = [track(2, "eng", "English", false, false)];
    let choice = hosts_choice(&host);
    assert_eq!(choice.subtitle, None);
    let friend = [track(5, "eng", "English", false, true)];
    assert_eq!(
        follow(PartySync::default(), &choice, &[], &friend).subtitle,
        Some(None)
    );
}

#[test]
fn a_friend_with_sync_off_keeps_their_subtitle() {
    let host = [track(7, "eng", "English", false, true)];
    let choice = hosts_choice(&host);
    let friend = [
        track(1, "fra", "French", false, true),
        track(2, "eng", "English", false, false),
    ];
    let sync = PartySync {
        subtitles: false,
        audio: true,
    };
    assert_eq!(follow(sync, &choice, &[], &friend).subtitle, None);
    assert_eq!(
        follow(PartySync::default(), &choice, &[], &friend).subtitle,
        Some(Some(2))
    );
}

#[test]
fn sync_settings_survive_a_restart() {
    let store = panorama_core::store::Store::open_in_memory().expect("store");
    assert_eq!(PartySync::load(Some(&store)), PartySync::default());
    let sync = PartySync {
        subtitles: false,
        audio: true,
    };
    sync.save(Some(&store));
    assert_eq!(PartySync::load(Some(&store)), sync);
    assert_eq!(PartySync::load(None), PartySync::default());
}

#[test]
fn an_unmatched_subtitle_does_not_revert_the_friends_audio() {
    let audio = |id, lang: &str, selected| track(id, lang, lang, false, selected);
    let host_subs = [track(2, "ind", "Indonesian", false, true)];
    let choice = choice_of(&[audio(1, "eng", true)], &host_subs).unwrap();
    let friend_subs = [track(5, "fra", "French", false, true)];

    let mut followed = Followed::new(choice, PartySync::default());
    let friend_audio = [audio(7, "jpn", true), audio(8, "eng", false)];
    let first = followed.step(&friend_audio, &friend_subs);
    assert_eq!(first.audio, Some(8));
    assert_eq!(first.subtitle, None, "no Indonesian here: keep their own");

    let chose_japanese = [audio(7, "jpn", true), audio(8, "eng", false)];
    for _ in 0..3 {
        assert_eq!(
            followed.step(&chose_japanese, &friend_subs),
            Follow::default(),
            "the unmatched subtitle must not pull the audio back"
        );
    }
}

#[test]
fn a_subtitle_that_turns_up_later_is_followed_once() {
    let host_subs = [track(2, "eng", "OpenSubtitles", true, true)];
    let choice = choice_of(&[], &host_subs).unwrap();
    let mut followed = Followed::new(choice, PartySync::default());
    let early = [track(1, "fra", "French", false, true)];
    assert_eq!(followed.step(&[], &early), Follow::default());
    let late = [
        track(1, "fra", "French", false, true),
        track(6, "eng", "OpenSubtitles", true, false),
    ];
    assert_eq!(followed.step(&[], &late).subtitle, Some(Some(6)));
    assert_eq!(followed.step(&[], &early), Follow::default());
}

fn hosts_release(torrent: Option<TorrentRef>) -> PartyRelease {
    PartyRelease {
        addon: "com.host.addon".into(),
        torrent,
        label: "Host addon\nFilm 1080p".into(),
        binge_group: None,
    }
}

#[test]
fn no_match_with_a_host_torrent_plays_the_torrent() {
    let release = hosts_release(Some(TorrentRef {
        info_hash: HASH.into(),
        file_idx: Some(2),
    }));
    let mut groups = vec![group("com.other#a", vec![link("Another film")])];
    let picked = Pick::Party(release.clone())
        .settle(&mut groups, None)
        .expect("a source");
    let chosen = &groups[picked.group];
    let StreamState::Ready(streams) = &chosen.state else {
        panic!("the torrent's group is ready");
    };
    let stream = &streams[picked.stream];
    assert!(matches!(resolve(stream), Ok(ResolvedSource::Torrent(_))));
    assert!(
        release_matches(&release, chosen, stream),
        "the roster shows the friend as matched"
    );
}

#[test]
fn no_match_without_a_host_torrent_plays_the_closest() {
    let mut groups = vec![group("com.other#a", vec![link("Another film")])];
    let picked = Pick::Party(hosts_release(None)).settle(&mut groups, None);
    assert_eq!(
        picked,
        Some(SourceRef {
            group: 0,
            stream: 0
        })
    );
    assert_eq!(groups.len(), 1);
}
