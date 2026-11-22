use super::*;
use crate::party::transport::PartySecret;

const SAM: ConnId = ConnId(1);
const KIM: ConnId = ConnId(2);
const LENGTH: f64 = 7_200.0;

fn host() -> HostSession {
    HostSession::new("Alex".into(), secret(), HostSettings::default())
}

fn hello(host: &mut HostSession, now: i64, conn: ConnId, secret: &str) -> Vec<HostOutput> {
    host.handle(now, HostInput::Connected { conn });
    host.handle(
        now,
        HostInput::Message {
            conn,
            msg: Up::Hello {
                name: format!("M{}", conn.0),
                version: PROTOCOL_VERSION,
                secret: PartySecret::from_hex(secret).unwrap(),
            },
        },
    )
}

fn join(host: &mut HostSession, now: i64, conn: ConnId, name: &str) -> Vec<HostOutput> {
    host.handle(now, HostInput::Connected { conn });
    host.handle(
        now,
        HostInput::Message {
            conn,
            msg: Up::Hello {
                name: name.into(),
                version: PROTOCOL_VERSION,
                secret: secret(),
            },
        },
    )
}

fn report(host: &mut HostSession, now: i64, conn: ConnId, state: MemberState) -> Vec<HostOutput> {
    report_with(host, now, conn, state, LENGTH, true)
}

fn report_with(
    host: &mut HostSession,
    now: i64,
    conn: ConnId,
    state: MemberState,
    duration: f64,
    release_matched: bool,
) -> Vec<HostOutput> {
    host.handle(
        now,
        HostInput::Message {
            conn,
            msg: Up::Report {
                state,
                position: 0.0,
                duration: Some(duration),
                release_matched,
            },
        },
    )
}

fn local(host: &mut HostSession, now: i64, state: MemberState) -> Vec<HostOutput> {
    host.handle(
        now,
        HostInput::LocalReport {
            state,
            position: 0.0,
            duration: Some(LENGTH),
        },
    )
}

fn request(host: &mut HostSession, now: i64, conn: ConnId, action: Action) -> Vec<HostOutput> {
    host.handle(
        now,
        HostInput::Message {
            conn,
            msg: Up::Request { action },
        },
    )
}

fn tick(host: &mut HostSession, now: i64) -> Vec<HostOutput> {
    host.handle(now, HostInput::Tick)
}

fn sent_to(out: &[HostOutput], to: ConnId) -> Vec<Down> {
    out.iter()
        .filter_map(|output| match output {
            HostOutput::Send { conn, msg } if *conn == to => Some(msg.clone()),
            _ => None,
        })
        .collect()
}

/// The `state` messages sent to `to`, as `(revision, playback, anyone_controls)`.
fn states(out: &[HostOutput], to: ConnId) -> Vec<(u64, PlaybackState, bool)> {
    sent_to(out, to)
        .into_iter()
        .filter_map(|msg| match msg {
            Down::State {
                revision,
                playback,
                anyone_controls,
                ..
            } => Some((revision, playback, anyone_controls)),
            _ => None,
        })
        .collect()
}

fn playing(out: &[HostOutput], to: ConnId) -> Option<bool> {
    states(out, to)
        .last()
        .map(|(_, playback, _)| playback.playing)
}

fn notices(out: &[HostOutput]) -> Vec<Notice> {
    out.iter()
        .filter_map(|output| match output {
            HostOutput::Notice(notice) => Some(notice.clone()),
            _ => None,
        })
        .collect()
}

fn roster(out: &[HostOutput]) -> Option<Vec<MemberView>> {
    out.iter().rev().find_map(|output| match output {
        HostOutput::Roster(members) => Some(members.clone()),
        _ => None,
    })
}

fn dropped(out: &[HostOutput], conn: ConnId) -> bool {
    out.contains(&HostOutput::Drop { conn })
}

/// Sets the media and leaves the lobby, if the party is still in it.
fn set_media(host: &mut HostSession, now: i64, at: f64) -> Vec<HostOutput> {
    let mut out = host.handle(now, HostInput::SetMedia { media: media(), at });
    out.extend(host.handle(now, HostInput::Start));
    out
}

/// Alex hosts, Sam has joined, and the film plays from 100 s since host time 0.
fn party() -> HostSession {
    let mut host = host();
    join(&mut host, 0, SAM, "Sam");
    set_media(&mut host, 0, 100.0);
    local(&mut host, 0, MemberState::Ready);
    let out = report(&mut host, 0, SAM, MemberState::Ready);
    assert_eq!(playing(&out, SAM), Some(true));
    host
}

/// `party()` with Sam's buffering holding it since 2_500.
fn held_by_sam() -> HostSession {
    let mut host = party();
    report(&mut host, 500, SAM, MemberState::Buffering);
    let out = tick(&mut host, 2_500);
    assert_eq!(playing(&out, SAM), Some(false));
    host
}

#[test]
fn right_secret_is_welcomed() {
    let mut host = host();
    let out = hello(&mut host, 0, SAM, SECRET);
    assert_eq!(sent_to(&out, SAM)[0], Down::Welcome { member_id: 1 });
    assert!(!dropped(&out, SAM));
}

#[test]
fn wrong_secret_gets_bye_and_drop() {
    let mut host = host();
    let out = hello(&mut host, 0, SAM, "ffffffffffffffffffffffffffffffff");
    assert_eq!(
        sent_to(&out, SAM),
        vec![Down::Bye {
            reason: ByeReason::BadSecret
        }]
    );
    assert!(dropped(&out, SAM));
}

#[test]
fn version_mismatch_gets_bye() {
    let mut host = host();
    host.handle(0, HostInput::Connected { conn: SAM });
    let out = host.handle(
        0,
        HostInput::Message {
            conn: SAM,
            msg: Up::Hello {
                name: "Sam".into(),
                version: PROTOCOL_VERSION + 1,
                secret: secret(),
            },
        },
    );
    assert_eq!(
        sent_to(&out, SAM),
        vec![Down::Bye {
            reason: ByeReason::VersionMismatch
        }]
    );
    assert!(dropped(&out, SAM));
}

#[test]
fn ninth_member_gets_bye_full() {
    let mut host = host();
    for id in 1..MAX_MEMBERS as u32 {
        let out = hello(&mut host, 0, ConnId(id), SECRET);
        assert!(!dropped(&out, ConnId(id)), "member {id} fits");
    }
    let last = ConnId(MAX_MEMBERS as u32);
    let out = hello(&mut host, 0, last, SECRET);
    assert_eq!(
        sent_to(&out, last),
        vec![Down::Bye {
            reason: ByeReason::Full
        }]
    );
    assert!(dropped(&out, last));
}

#[test]
fn first_message_must_be_hello() {
    let mut host = host();
    host.handle(0, HostInput::Connected { conn: SAM });
    let out = host.handle(
        0,
        HostInput::Message {
            conn: SAM,
            msg: Up::Ping { id: 0, sent_ms: 0 },
        },
    );
    assert!(dropped(&out, SAM));
    assert!(sent_to(&out, SAM).is_empty());
}

#[test]
fn silent_connection_is_dropped_after_10s() {
    let mut host = host();
    host.handle(0, HostInput::Connected { conn: SAM });
    assert!(!dropped(&tick(&mut host, 10_000), SAM));
    assert!(dropped(&tick(&mut host, 10_001), SAM));
}

#[test]
fn pong_echoes_sent_ms_with_host_time() {
    let mut host = host();
    join(&mut host, 0, SAM, "Sam");
    let out = host.handle(
        5_000,
        HostInput::Message {
            conn: SAM,
            msg: Up::Ping {
                id: 9,
                sent_ms: 123,
            },
        },
    );
    assert_eq!(
        sent_to(&out, SAM),
        vec![Down::Pong {
            id: 9,
            sent_ms: 123,
            host_ms: 5_000
        }]
    );
}

#[test]
fn joining_mid_film_gets_load_and_state() {
    let mut host = party();
    let out = join(&mut host, 3_000, KIM, "Kim");
    let sent = sent_to(&out, KIM);
    assert_eq!(sent[0], Down::Welcome { member_id: 2 });
    assert!(matches!(&sent[1], Down::Load { media: m, .. } if *m == media()));
    assert!(matches!(sent[2], Down::State { playback, .. } if playback.playing));
    assert_eq!(notices(&out)[0].kind, NoticeKind::Joined);
    assert_eq!(roster(&out).map(|r| r.len()), Some(3));
}

#[test]
fn load_reaches_every_member_and_opens_paused_at_the_start() {
    let mut host = host();
    join(&mut host, 0, SAM, "Sam");
    let out = set_media(&mut host, 1_000, 42.0);
    assert!(matches!(&sent_to(&out, SAM)[0], Down::Load { .. }));
    let expected = PlaybackState {
        playing: false,
        anchor_position: 42.0,
        anchor_host_ms: 1_000,
    };
    assert_eq!(states(&out, SAM)[0].1, expected);
    assert!(out.contains(&HostOutput::Apply(expected)));
}

#[test]
fn play_waits_for_all_ready() {
    let mut host = host();
    join(&mut host, 0, SAM, "Sam");
    set_media(&mut host, 0, 0.0);
    local(&mut host, 100, MemberState::Ready);
    let out = host.handle(200, HostInput::Local(Action::Play));
    assert_ne!(playing(&out, SAM), Some(true));
    let out = report(&mut host, 300, SAM, MemberState::Ready);
    assert_eq!(playing(&out, SAM), Some(true));
}

#[test]
fn play_starts_after_20s_even_if_not_ready() {
    let mut host = host();
    join(&mut host, 0, SAM, "Sam");
    set_media(&mut host, 0, 0.0);
    local(&mut host, 0, MemberState::Ready);
    assert_ne!(playing(&tick(&mut host, 20_000), SAM), Some(true));
    let out = tick(&mut host, 20_001);
    assert_eq!(playing(&out, SAM), Some(true));
    let out = tick(&mut host, 20_250);
    let sam = roster(&out).unwrap().into_iter().find(|m| m.name == "Sam");
    assert!(sam.unwrap().late);
}

#[test]
fn failed_member_does_not_block_start() {
    let mut host = host();
    join(&mut host, 0, SAM, "Sam");
    set_media(&mut host, 0, 0.0);
    report(&mut host, 0, SAM, MemberState::Failed);
    let out = local(&mut host, 100, MemberState::Ready);
    assert_eq!(playing(&out, SAM), Some(true));
}

#[test]
fn buffering_over_1500ms_pauses_party() {
    let mut host = party();
    report(&mut host, 500, SAM, MemberState::Buffering);
    assert_ne!(playing(&tick(&mut host, 2_000), SAM), Some(false));
    let out = tick(&mut host, 2_500);
    let (_, playback, _) = states(&out, SAM)[0];
    assert!(!playback.playing);
    assert_eq!(playback.anchor_position, 102.5);
    assert_eq!(
        notices(&out),
        vec![Notice {
            kind: NoticeKind::WaitingFor,
            by: "Sam".into(),
            at: Some(102.5)
        }]
    );
}

#[test]
fn a_wait_pauses_where_the_buffering_player_stopped() {
    let mut host = party();
    host.handle(
        500,
        HostInput::Message {
            conn: SAM,
            msg: Up::Report {
                state: MemberState::Buffering,
                position: 100.4,
                duration: Some(LENGTH),
                release_matched: true,
            },
        },
    );
    let out = tick(&mut host, 2_500);
    let (_, playback, _) = states(&out, SAM)[0];
    assert!(!playback.playing);
    assert_eq!(playback.anchor_position, 100.4);
}

#[test]
fn resumes_when_all_playing() {
    let mut host = held_by_sam();
    let out = report(&mut host, 4_000, SAM, MemberState::Paused);
    let (_, playback, _) = states(&out, SAM)[0];
    assert!(playback.playing);
    assert_eq!(playback.anchor_host_ms, 4_000);
}

#[test]
fn host_buffering_holds_the_party_too() {
    let mut host = party();
    local(&mut host, 500, MemberState::Buffering);
    assert_eq!(playing(&tick(&mut host, 2_001), SAM), Some(false));
}

#[test]
fn failed_member_does_not_block_buffer_wait() {
    let mut host = party();
    join(&mut host, 0, KIM, "Kim");
    report(&mut host, 0, KIM, MemberState::Failed);
    report(&mut host, 500, SAM, MemberState::Buffering);
    tick(&mut host, 2_001);
    let out = report(&mut host, 3_000, SAM, MemberState::Playing);
    assert_eq!(playing(&out, SAM), Some(true));
}

#[test]
fn buffer_wait_gives_up_after_30s() {
    let mut host = held_by_sam();
    assert_ne!(playing(&tick(&mut host, 32_500), SAM), Some(true));
    assert_eq!(playing(&tick(&mut host, 32_501), SAM), Some(true));
    assert_ne!(
        playing(&tick(&mut host, 40_000), SAM),
        Some(false),
        "a member who outlasted the limit no longer holds the party"
    );
}

#[test]
fn member_holds_party_at_most_three_times_per_media() {
    let mut host = party();
    let mut now = 0;
    for wait in 1..=4 {
        report(&mut host, now, SAM, MemberState::Buffering);
        let held = playing(&tick(&mut host, now + 1_501), SAM) == Some(false);
        assert_eq!(held, wait <= 3, "wait {wait}");
        report(&mut host, now + 2_000, SAM, MemberState::Playing);
        now += 10_000;
    }
}

#[test]
fn load_resets_wait_counts() {
    let mut host = party();
    let mut now = 0;
    for _ in 0..3 {
        report(&mut host, now, SAM, MemberState::Buffering);
        tick(&mut host, now + 1_501);
        report(&mut host, now + 2_000, SAM, MemberState::Playing);
        now += 10_000;
    }
    set_media(&mut host, now, 0.0);
    local(&mut host, now, MemberState::Ready);
    report(&mut host, now, SAM, MemberState::Ready);
    report(&mut host, now + 100, SAM, MemberState::Buffering);
    assert_eq!(playing(&tick(&mut host, now + 1_601), SAM), Some(false));
}

#[test]
fn wait_for_everyone_off_never_holds() {
    let mut host = party();
    host.handle(
        0,
        HostInput::Settings(HostSettings {
            wait_for_everyone: false,
            anyone_controls: false,
        }),
    );
    report(&mut host, 500, SAM, MemberState::Buffering);
    assert_ne!(playing(&tick(&mut host, 2_001), SAM), Some(false));
}

#[test]
fn duration_mismatch_over_2s_sends_notice() {
    let mut host = party();
    let out = report_with(
        &mut host,
        1_000,
        SAM,
        MemberState::Playing,
        LENGTH + 2.0,
        true,
    );
    assert!(notices(&out).is_empty());
    let out = report_with(
        &mut host,
        2_000,
        SAM,
        MemberState::Playing,
        LENGTH + 2.5,
        true,
    );
    let notice = &notices(&out)[0];
    assert_eq!(
        (notice.kind, notice.by.as_str()),
        (NoticeKind::DifferentLength, "Sam")
    );
    assert!(sent_to(&out, SAM).contains(&Down::Notice(notice.clone())));
    let out = report_with(
        &mut host,
        3_000,
        SAM,
        MemberState::Playing,
        LENGTH + 3.0,
        true,
    );
    assert!(notices(&out).is_empty(), "told once per media");
}

#[test]
fn different_release_sends_notice_and_shows_in_roster() {
    let mut host = party();
    let out = report_with(&mut host, 1_000, SAM, MemberState::Playing, LENGTH, false);
    assert_eq!(notices(&out)[0].kind, NoticeKind::DifferentRelease);
    let sam = roster(&out).unwrap().into_iter().find(|m| m.name == "Sam");
    assert!(!sam.unwrap().release_matched);
}

#[test]
fn member_request_ignored_when_host_only() {
    let mut host = party();
    tick(&mut host, 500);
    let out = request(&mut host, 1_000, SAM, Action::Pause);
    assert!(states(&out, SAM).is_empty());
    assert_eq!(
        sent_to(&out, SAM),
        vec![Down::Notice(Notice {
            kind: NoticeKind::Denied,
            by: "Alex".into(),
            at: None
        })]
    );
}

#[test]
fn member_request_applied_when_anyone_controls() {
    let mut host = party();
    let out = host.handle(
        0,
        HostInput::Settings(HostSettings {
            wait_for_everyone: true,
            anyone_controls: true,
        }),
    );
    let (revision, ..) = states(&out, SAM)[0];
    let out = request(&mut host, 4_000, SAM, Action::Seek { to: 600.0 });
    let (next, playback, _) = states(&out, SAM)[0];
    assert_eq!(next, revision + 1);
    assert_eq!(playback.anchor_position, 600.0);
    assert_eq!(playback.anchor_host_ms, 4_000);
    assert_eq!(
        notices(&out),
        vec![Notice {
            kind: NoticeKind::Seeked,
            by: "Sam".into(),
            at: Some(600.0)
        }]
    );
}

#[test]
fn members_see_anyone_controls() {
    let mut host = party();
    let out = host.handle(
        0,
        HostInput::Settings(HostSettings {
            wait_for_everyone: true,
            anyone_controls: true,
        }),
    );
    assert!(states(&out, SAM)[0].2);
    assert!(!out.iter().any(|o| matches!(o, HostOutput::Apply(_))));
}

#[test]
fn state_refresh_every_5s() {
    let mut host = party();
    assert!(states(&tick(&mut host, 4_999), SAM).is_empty());
    let refresh = states(&tick(&mut host, 5_000), SAM);
    assert_eq!(refresh.len(), 1);
    assert!(states(&tick(&mut host, 9_999), SAM).is_empty());
    assert_eq!(states(&tick(&mut host, 10_000), SAM), refresh);
}

#[test]
fn pause_anchor_freezes_expected_position() {
    let mut host = party();
    let out = host.handle(5_000, HostInput::Local(Action::Pause));
    let (_, playback, _) = states(&out, SAM)[0];
    assert!(!playback.playing);
    assert_eq!(playback.anchor_position, 105.0);
    assert_eq!(playback.expected_at(60_000), 105.0);
    assert!(out.contains(&HostOutput::Apply(playback)));
    assert_eq!(notices(&out)[0].at, Some(105.0));
}

#[test]
fn last_request_wins() {
    let mut host = party();
    host.handle(1_000, HostInput::Local(Action::Pause));
    host.handle(1_050, HostInput::Local(Action::Play));
    let out = host.handle(1_100, HostInput::Local(Action::Seek { to: 30.0 }));
    let (_, playback, _) = states(&out, SAM)[0];
    assert!(playback.playing);
    assert_eq!(playback.anchor_position, 30.0);
}

#[test]
fn leaving_member_is_announced() {
    let mut host = party();
    let out = host.handle(1_000, HostInput::Disconnected { conn: SAM });
    assert_eq!(notices(&out)[0].kind, NoticeKind::Left);
    assert_eq!(roster(&out).map(|r| r.len()), Some(1));
}

#[test]
fn end_says_host_left_and_drops_everyone() {
    let mut host = party();
    let out = host.handle(1_000, HostInput::End);
    assert_eq!(
        sent_to(&out, SAM),
        vec![Down::Bye {
            reason: ByeReason::HostLeft
        }]
    );
    assert!(dropped(&out, SAM));
    assert!(tick(&mut host, 2_000).is_empty());
}

fn local_at(host: &mut HostSession, now: i64, position: f64) -> Vec<HostOutput> {
    host.handle(
        now,
        HostInput::LocalReport {
            state: MemberState::Playing,
            position,
            duration: Some(LENGTH),
        },
    )
}

#[test]
fn a_host_behind_its_own_timeline_moves_the_timeline_to_it() {
    let mut host = party();
    // Playing from 100 s at t=0, so 110 s is expected at t=10_000; the host's player is at 109.
    let out = local_at(&mut host, 10_000, 109.0);
    let (_, playback, _) = states(&out, SAM)[0];
    assert!(playback.playing);
    assert_eq!(playback.anchor_position, 109.0);
    assert_eq!(playback.anchor_host_ms, 10_000);
}

#[test]
fn a_small_host_lag_leaves_the_timeline_alone() {
    let mut host = party();
    let out = local_at(&mut host, 10_000, 109.8);
    assert!(
        states(&out, SAM)
            .iter()
            .all(|(_, playback, _)| playback.anchor_position == 100.0)
    );
}

#[test]
fn a_report_from_before_a_seek_does_not_undo_it() {
    let mut host = party();
    host.handle(10_000, HostInput::Local(Action::Seek { to: 500.0 }));
    // The player has not caught up with the seek yet.
    let out = local_at(&mut host, 10_500, 110.4);
    assert!(
        states(&out, SAM)
            .iter()
            .all(|(_, playback, _)| playback.anchor_position == 500.0)
    );
}

fn every_state_decodes(out: &[HostOutput]) {
    for (_, playback, anyone_controls) in states(out, SAM) {
        let line = encode(&Down::State {
            revision: 1,
            playback,
            anyone_controls,
            gate: Gate::Ready,
            tracks: None,
        });
        assert!(decode_down(&line).is_ok(), "members refuse {playback:?}");
    }
}

#[test]
fn the_timeline_never_leaves_what_members_accept() {
    let mut host = host();
    join(&mut host, 0, SAM, "Sam");
    let out = set_media(&mut host, 0, 100_000.0);
    every_state_decodes(&out);
    assert_eq!(states(&out, SAM)[0].1.anchor_position, 86_400.0);

    let mut host = party();
    let out = host.handle(200_000_000, HostInput::Local(Action::Pause));
    every_state_decodes(&out);
}

#[test]
fn the_timeline_stops_at_the_end_of_the_media() {
    let mut host = party();
    local_at(&mut host, 1_000, 101.0);
    let out = host.handle(20_000_000, HostInput::Local(Action::Pause));
    assert_eq!(states(&out, SAM)[0].1.anchor_position, LENGTH);
}

#[test]
fn a_member_named_like_the_host_gets_a_suffix() {
    let mut host = host();
    let out = join(&mut host, 0, SAM, "ALEX");
    assert_eq!(sent_to(&out, SAM)[0], Down::Welcome { member_id: 1 });
    let names = roster(&out)
        .unwrap()
        .into_iter()
        .map(|member| member.name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["Alex", "ALEX (2)"]);
    assert_eq!(notices(&out)[0].by, "ALEX (2)");
    let out = join(&mut host, 300, KIM, "alex");
    assert_eq!(roster(&out).unwrap()[2].name, "alex (3)");

    let long = "x".repeat(32);
    let mut host = HostSession::new(long.clone(), secret(), HostSettings::default());
    let out = join(&mut host, 0, SAM, &long);
    let sam = &roster(&out).unwrap()[1].name;
    assert_eq!(*sam, format!("{} (2)", "x".repeat(28)));
    assert!(
        decode_down(&encode(&Down::Roster {
            members: roster(&out).unwrap()
        }))
        .is_ok()
    );
}

#[test]
fn a_flapping_member_moves_the_roster_at_most_once_a_tick() {
    let mut host = party();
    let mut rosters = 0;
    for step in 0..20 {
        let state = if step % 2 == 0 {
            MemberState::Buffering
        } else {
            MemberState::Playing
        };
        let out = report(&mut host, 1_000 + step * 10, SAM, state);
        rosters += sent_to(&out, SAM)
            .iter()
            .filter(|msg| matches!(msg, Down::Roster { .. }))
            .count();
    }
    assert!(rosters <= 1, "{rosters} rosters in 200 ms");
    let out = tick(&mut host, 1_260);
    let sam = roster(&out).unwrap().into_iter().find(|m| m.name == "Sam");
    assert_eq!(sam.unwrap().state, MemberState::Playing);
    assert!(roster(&tick(&mut host, 1_520)).is_none());
}

/// Alex hosts with Sam joined, and the film is set while the party is still in the lobby.
fn lobby() -> (HostSession, Vec<HostOutput>) {
    let mut host = host();
    join(&mut host, 0, SAM, "Sam");
    let out = host.handle(
        1_000,
        HostInput::SetMedia {
            media: media(),
            at: 42.0,
        },
    );
    (host, out)
}

fn gate(out: &[HostOutput], to: ConnId) -> Option<Gate> {
    sent_to(out, to)
        .into_iter()
        .rev()
        .find_map(|msg| match msg {
            Down::State { gate, .. } => Some(gate),
            _ => None,
        })
}

fn tracks(out: &[HostOutput], to: ConnId) -> Option<Option<TrackChoice>> {
    sent_to(out, to)
        .into_iter()
        .rev()
        .find_map(|msg| match msg {
            Down::State { tracks, .. } => Some(tracks),
            _ => None,
        })
}

#[test]
fn a_new_party_waits_in_the_lobby_until_start() {
    let (mut host, _) = lobby();
    local(&mut host, 1_500, MemberState::Ready);
    report(&mut host, 1_500, SAM, MemberState::Ready);
    host.handle(2_000, HostInput::Local(Action::Play));
    let out = tick(&mut host, 60_000);
    assert_eq!(playing(&out, SAM), Some(false));
    let out = host.handle(60_000, HostInput::Start);
    assert_eq!(playing(&out, SAM), Some(true));
}

#[test]
fn start_leaves_the_lobby_and_waits_for_members_still_loading() {
    let (mut host, out) = lobby();
    assert_eq!(gate(&out, SAM), Some(Gate::Lobby));
    local(&mut host, 1_500, MemberState::Ready);
    let out = host.handle(2_000, HostInput::Start);
    assert_eq!(gate(&out, SAM), Some(Gate::Ready));
    assert_eq!(playing(&out, SAM), Some(false));
    let out = report(&mut host, 3_000, SAM, MemberState::Ready);
    assert_eq!(playing(&out, SAM), Some(true));
}

#[test]
fn set_media_in_the_lobby_does_not_start_playback() {
    let (mut host, out) = lobby();
    assert!(matches!(&sent_to(&out, SAM)[0], Down::Load { media: m, .. } if *m == media()));
    let paused = PlaybackState {
        playing: false,
        anchor_position: 42.0,
        anchor_host_ms: 1_000,
    };
    assert_eq!(states(&out, SAM)[0].1, paused);
    let out = report(&mut host, 1_500, SAM, MemberState::Ready);
    assert_eq!(roster(&out).unwrap()[1].state, MemberState::Ready);
    host.handle(2_000, HostInput::Local(Action::Pause));
    let out = host.handle(
        3_000,
        HostInput::SetMedia {
            media: media(),
            at: 0.0,
        },
    );
    assert_eq!(playing(&out, SAM), Some(false));
    let out = tick(&mut host, 40_000);
    assert_eq!(playing(&out, SAM), Some(false));
    assert!(
        !out.iter()
            .any(|o| matches!(o, HostOutput::Apply(p) if p.playing))
    );
}

#[test]
fn state_names_the_member_the_party_waits_for() {
    let mut host = party();
    report(&mut host, 500, SAM, MemberState::Buffering);
    let out = tick(&mut host, 2_500);
    assert_eq!(gate(&out, SAM), Some(Gate::Waiting(1)));
    let out = report(&mut host, 3_000, SAM, MemberState::Playing);
    assert_eq!(gate(&out, SAM), Some(Gate::Ready));
}

#[test]
fn a_version_1_member_gets_version_mismatch() {
    let mut host = host();
    host.handle(0, HostInput::Connected { conn: SAM });
    let hello =
        r#"{"t":"hello","name":"Sam","version":1,"secret":"0123456789abcdef0123456789abcdef"}"#;
    let out = host.handle(
        0,
        HostInput::Message {
            conn: SAM,
            msg: decode_up(hello).unwrap(),
        },
    );
    assert_eq!(
        sent_to(&out, SAM),
        vec![Down::Bye {
            reason: ByeReason::VersionMismatch
        }]
    );
    assert!(dropped(&out, SAM));
}

#[test]
fn the_hosts_tracks_reach_members() {
    let mut host = party();
    let out = host.handle(1_000, HostInput::Tracks(english_subtitles()));
    assert_eq!(tracks(&out, SAM), Some(Some(english_subtitles())));
    let mut long = english_subtitles();
    long.subtitle.as_mut().unwrap().title = Some(format!("Eng\u{200B}lish{}", "t".repeat(300)));
    let out = host.handle(2_000, HostInput::Tracks(long));
    let sent = tracks(&out, SAM)
        .flatten()
        .and_then(|tracks| tracks.subtitle);
    let title = sent.and_then(|key| key.title).unwrap();
    assert_eq!(title.chars().count(), 200);
    assert!(title.starts_with("Englishttt"), "{title}");
    every_state_decodes(&out);
}

#[test]
fn new_media_forgets_the_hosts_tracks() {
    let mut host = party();
    host.handle(1_000, HostInput::Tracks(english_subtitles()));
    let out = set_media(&mut host, 2_000, 0.0);
    assert_eq!(tracks(&out, SAM), Some(None));
}

#[test]
fn the_lobby_holds_on_pause_and_moves_on_seek() {
    let (mut host, _) = lobby();
    let out = host.handle(2_000, HostInput::Local(Action::Pause));
    assert!(sent_to(&out, SAM).is_empty());
    let out = host.handle(3_000, HostInput::Local(Action::Seek { to: 500.0 }));
    let (_, playback, _) = *states(&out, SAM).last().unwrap();
    assert_eq!(playback.anchor_position, 500.0);
    assert!(!playback.playing);
    assert_eq!(gate(&out, SAM), Some(Gate::Lobby));
}

#[test]
fn a_members_play_in_the_lobby_is_denied() {
    let (mut host, _) = lobby();
    host.handle(
        1_500,
        HostInput::Settings(HostSettings {
            anyone_controls: true,
            ..HostSettings::default()
        }),
    );
    let out = request(&mut host, 2_000, SAM, Action::Play);
    assert!(sent_to(&out, SAM).iter().any(
        |msg| matches!(msg, Down::Notice(Notice { kind, .. }) if *kind == NoticeKind::Denied)
    ));
    assert_eq!(playing(&out, SAM), None);
}

#[test]
fn a_member_joining_the_lobby_gets_the_media_and_the_lobby() {
    let (mut host, _) = lobby();
    let out = join(&mut host, 2_000, KIM, "Kim");
    let sent = sent_to(&out, KIM);
    assert!(matches!(&sent[1], Down::Load { media: m, .. } if *m == media()));
    assert!(matches!(
        sent[2],
        Down::State { gate: Gate::Lobby, playback, .. } if !playback.playing
    ));
}

#[test]
fn start_before_media_does_nothing() {
    let mut host = host();
    join(&mut host, 0, SAM, "Sam");
    local(&mut host, 0, MemberState::Ready);
    host.handle(500, HostInput::Start);
    let out = host.handle(
        1_000,
        HostInput::SetMedia {
            media: media(),
            at: 0.0,
        },
    );
    assert_eq!(gate(&out, SAM), Some(Gate::Lobby));
    report(&mut host, 1_500, SAM, MemberState::Ready);
    let out = tick(&mut host, 30_000);
    assert_eq!(playing(&out, SAM), Some(false));
}

fn stops(out: &[HostOutput], to: ConnId) -> usize {
    sent_to(out, to)
        .iter()
        .filter(|msg| matches!(msg, Down::Stop { .. }))
        .count()
}

#[test]
fn the_host_leaving_the_film_sends_everyone_back_to_the_lobby() {
    let mut host = party();
    let out = host.handle(5_000, HostInput::Stop);
    assert_eq!(stops(&out, SAM), 1);
    assert_eq!(host.gate(), Gate::Lobby);
    assert!(states(&tick(&mut host, 20_000), SAM).is_empty());

    let out = host.handle(
        21_000,
        HostInput::SetMedia {
            media: media(),
            at: 0.0,
        },
    );
    assert!(
        sent_to(&out, SAM)
            .iter()
            .any(|msg| matches!(msg, Down::Load { .. }))
    );
    assert_eq!(gate(&out, SAM), Some(Gate::Lobby));
}

#[test]
fn stopping_twice_tells_the_party_once() {
    let mut host = party();
    host.handle(5_000, HostInput::Stop);
    assert_eq!(stops(&host.handle(6_000, HostInput::Stop), SAM), 0);
}

#[test]
fn a_friend_joining_after_the_stop_waits_in_the_lobby_with_nothing_to_load() {
    let mut host = party();
    host.handle(5_000, HostInput::Stop);
    let out = join(&mut host, 6_000, KIM, "Kim");
    let sent = sent_to(&out, KIM);
    assert!(sent.iter().any(|msg| matches!(msg, Down::Welcome { .. })));
    assert!(
        !sent
            .iter()
            .any(|msg| matches!(msg, Down::Load { .. } | Down::State { .. }))
    );
}

#[test]
fn ending_after_a_stop_still_says_goodbye() {
    let mut host = party();
    host.handle(5_000, HostInput::Stop);
    let out = host.handle(6_000, HostInput::End);
    assert!(sent_to(&out, SAM).contains(&Down::Bye {
        reason: ByeReason::HostLeft
    }));
}

fn waits(out: &[HostOutput]) -> usize {
    notices(out)
        .iter()
        .filter(|notice| notice.kind == NoticeKind::WaitingFor)
        .count()
}

/// `party()` once the host switched audio at 1_000 and both players stalled on the reload.
fn switched_audio() -> HostSession {
    let mut host = party();
    let mut french_audio = english_subtitles();
    french_audio.audio = Some(TrackKey {
        language: Some("fr".into()),
        title: None,
        codec: Some("aac".into()),
        external: false,
    });
    host.handle(500, HostInput::Tracks(english_subtitles()));
    host.handle(1_000, HostInput::Tracks(french_audio));
    local(&mut host, 1_100, MemberState::Buffering);
    report(&mut host, 1_300, SAM, MemberState::Buffering);
    host
}

#[test]
fn the_stall_after_a_track_switch_does_not_hold_the_party() {
    let mut host = switched_audio();
    let out = tick(&mut host, 3_500);
    assert_eq!(waits(&out), 0);
    local(&mut host, 3_600, MemberState::Playing);
    report(&mut host, 3_700, SAM, MemberState::Playing);
    assert_eq!(waits(&tick(&mut host, 8_000)), 0);
}

#[test]
fn a_stall_that_outlasts_the_track_switch_still_holds_the_party() {
    let mut host = switched_audio();
    tick(&mut host, 3_500);
    let out = tick(&mut host, 6_500);
    assert_eq!(waits(&out), 1);
}

fn loads(out: &[HostOutput], to: ConnId) -> usize {
    sent_to(out, to)
        .iter()
        .filter(|msg| matches!(msg, Down::Load { .. }))
        .count()
}

#[test]
fn a_source_picked_mid_film_reloads_everyone_where_the_host_is() {
    let mut host = party();
    let out = host.handle(
        30_000,
        HostInput::SetMedia {
            media: media(),
            at: 130.0,
        },
    );
    assert_eq!(loads(&out, SAM), 1);
    assert_eq!(playing(&out, SAM), Some(false));
    local(&mut host, 32_000, MemberState::Ready);
    let out = report(&mut host, 33_000, SAM, MemberState::Ready);
    assert_eq!(playing(&out, SAM), Some(true));
    let (_, playback, _) = *states(&out, SAM).last().unwrap();
    assert_eq!(playback.anchor_position, 130.0);
}

#[test]
fn a_source_picked_while_paused_stays_paused() {
    let mut host = party();
    host.handle(10_000, HostInput::Local(Action::Pause));
    host.handle(
        30_000,
        HostInput::SetMedia {
            media: media(),
            at: 110.0,
        },
    );
    local(&mut host, 32_000, MemberState::Ready);
    report(&mut host, 33_000, SAM, MemberState::Ready);
    let out = tick(&mut host, 60_000);
    assert_ne!(playing(&out, SAM), Some(true));
    assert_eq!(host.gate(), Gate::Ready);
}

#[test]
fn resuming_before_everyone_loaded_the_new_source_waits_for_them() {
    let mut host = party();
    host.handle(10_000, HostInput::Local(Action::Pause));
    host.handle(
        30_000,
        HostInput::SetMedia {
            media: media(),
            at: 110.0,
        },
    );
    local(&mut host, 31_000, MemberState::Ready);
    let out = host.handle(32_000, HostInput::Local(Action::Play));
    assert_ne!(playing(&out, SAM), Some(true));
    let out = report(&mut host, 34_000, SAM, MemberState::Ready);
    assert_eq!(playing(&out, SAM), Some(true));
}

#[test]
fn new_media_ends_the_track_switch_grace() {
    let mut host = switched_audio();
    host.handle(
        1_500,
        HostInput::SetMedia {
            media: media(),
            at: 0.0,
        },
    );
    local(&mut host, 1_600, MemberState::Ready);
    report(&mut host, 1_700, SAM, MemberState::Ready);
    tick(&mut host, 1_800);
    report(&mut host, 2_000, SAM, MemberState::Buffering);
    assert_eq!(waits(&tick(&mut host, 4_000)), 1);
}
