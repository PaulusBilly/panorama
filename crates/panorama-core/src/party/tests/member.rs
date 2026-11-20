use super::*;

fn member() -> MemberSession {
    MemberSession::new("Sam".into(), SECRET.into())
}

/// Joined at 0, with a clock that agrees with the host's.
fn joined() -> MemberSession {
    let mut member = member();
    member.handle(0, MemberInput::Start);
    member.handle(10, MemberInput::Down(Down::Welcome { member_id: 1 }));
    member.handle(
        50,
        MemberInput::Down(Down::Pong {
            id: 0,
            sent_ms: 0,
            host_ms: 25,
        }),
    );
    member
}

fn state(revision: u64, playing: bool, at: f64, host_ms: i64) -> MemberInput {
    MemberInput::Down(Down::State {
        revision,
        playback: PlaybackState {
            playing,
            anchor_position: at,
            anchor_host_ms: host_ms,
        },
        anyone_controls: false,
    })
}

fn player(state: MemberState, position: f64) -> MemberInput {
    MemberInput::Player {
        state,
        position,
        duration: Some(7_200.0),
    }
}

/// `joined()`, playing in step with a party that plays from 100 s since host time 0.
fn watching() -> MemberSession {
    let mut member = joined();
    member.handle(100, state(1, true, 100.0, 0));
    member.handle(2_000, player(MemberState::Playing, 102.0));
    member
}

fn player_commands(out: &[MemberOutput]) -> Vec<MemberOutput> {
    out.iter()
        .filter(|o| {
            matches!(
                o,
                MemberOutput::Seek(_) | MemberOutput::SetPaused(_) | MemberOutput::SetSpeed(_)
            )
        })
        .cloned()
        .collect()
}

fn reports(out: &[MemberOutput]) -> Vec<Up> {
    out.iter()
        .filter_map(|o| match o {
            MemberOutput::Send(up @ Up::Report { .. }) => Some(up.clone()),
            _ => None,
        })
        .collect()
}

fn speed(out: &[MemberOutput]) -> Option<f64> {
    out.iter().find_map(|o| match o {
        MemberOutput::SetSpeed(speed) => Some(*speed),
        _ => None,
    })
}

#[test]
fn hello_then_pings_on_start() {
    let out = member().handle(0, MemberInput::Start);
    assert_eq!(
        out,
        vec![
            MemberOutput::Send(Up::Hello {
                name: "Sam".into(),
                version: PROTOCOL_VERSION,
                secret: SECRET.into(),
            }),
            MemberOutput::Send(Up::Ping { id: 0, sent_ms: 0 }),
        ]
    );
}

#[test]
fn pings_every_2s_then_every_10s() {
    let mut member = member();
    let mut sent = Vec::new();
    for now in (0..=30_000).step_by(250) {
        let input = if now == 0 {
            MemberInput::Start
        } else {
            MemberInput::Tick
        };
        for out in member.handle(now, input) {
            if let MemberOutput::Send(Up::Ping { sent_ms, .. }) = out {
                sent.push(sent_ms);
            }
        }
    }
    assert_eq!(sent, vec![0, 2_000, 4_000, 6_000, 8_000, 18_000, 28_000]);
}

#[test]
fn welcome_is_surfaced() {
    let mut member = member();
    member.handle(0, MemberInput::Start);
    let out = member.handle(10, MemberInput::Down(Down::Welcome { member_id: 4 }));
    assert_eq!(out, vec![MemberOutput::Welcomed { member_id: 4 }]);
}

#[test]
fn load_opens_paused_at_anchor() {
    let mut member = joined();
    let out = member.handle(
        100,
        MemberInput::Down(Down::Load {
            media: media(),
            revision: 1,
        }),
    );
    assert_eq!(out, vec![MemberOutput::Load(media())]);
    let out = member.handle(200, state(2, false, 42.0, 0));
    assert_eq!(
        player_commands(&out),
        vec![MemberOutput::Seek(42.0), MemberOutput::SetPaused(true)]
    );
}

#[test]
fn state_playing_emits_seek_and_unpause() {
    let mut member = joined();
    let out = member.handle(1_000, state(1, true, 100.0, 0));
    assert_eq!(
        player_commands(&out),
        vec![MemberOutput::Seek(101.0), MemberOutput::SetPaused(false)]
    );
}

#[test]
fn playing_state_waits_for_the_clock() {
    let mut member = member();
    member.handle(0, MemberInput::Start);
    member.handle(10, MemberInput::Down(Down::Welcome { member_id: 1 }));
    let out = member.handle(500, state(1, true, 100.0, 0));
    assert!(player_commands(&out).is_empty());
    let out = member.handle(
        1_000,
        MemberInput::Down(Down::Pong {
            id: 0,
            sent_ms: 0,
            host_ms: 500,
        }),
    );
    assert_eq!(
        player_commands(&out),
        vec![MemberOutput::Seek(101.0), MemberOutput::SetPaused(false)]
    );
}

#[test]
fn pause_continues_without_seek() {
    let mut member = watching();
    let out = member.handle(5_000, state(2, false, 105.0, 5_000));
    assert_eq!(player_commands(&out), vec![MemberOutput::SetPaused(true)]);
}

#[test]
fn host_seek_moves_the_player() {
    let mut member = watching();
    let out = member.handle(5_000, state(2, true, 600.0, 5_000));
    assert_eq!(
        player_commands(&out),
        vec![MemberOutput::Seek(600.0), MemberOutput::SetPaused(false)]
    );
}

#[test]
fn stale_revision_is_ignored() {
    let mut member = watching();
    let out = member.handle(2_500, state(0, false, 50.0, 2_500));
    assert!(out.is_empty());
    let out = member.handle(
        2_600,
        MemberInput::Down(Down::Load {
            media: media(),
            revision: 0,
        }),
    );
    assert!(out.is_empty());
}

#[test]
fn state_refresh_does_not_touch_the_player() {
    let mut member = watching();
    let out = member.handle(5_000, state(1, true, 100.0, 0));
    assert!(player_commands(&out).is_empty());
}

#[test]
fn members_see_anyone_controls() {
    let mut member = joined();
    let update = |anyone_controls| {
        MemberInput::Down(Down::State {
            revision: 1,
            playback: PlaybackState {
                playing: false,
                anchor_position: 0.0,
                anchor_host_ms: 0,
            },
            anyone_controls,
        })
    };
    let out = member.handle(100, update(true));
    assert!(out.contains(&MemberOutput::Controls {
        anyone_controls: true
    }));
    let out = member.handle(200, update(true));
    assert!(
        !out.iter()
            .any(|o| matches!(o, MemberOutput::Controls { .. }))
    );
}

#[test]
fn local_pause_sends_request_not_set_paused() {
    let mut member = watching();
    let out = member.handle(2_100, MemberInput::Local(Action::Pause));
    assert_eq!(
        out,
        vec![MemberOutput::Send(Up::Request {
            action: Action::Pause
        })]
    );
}

#[test]
fn reports_every_second_while_playing() {
    let mut member = joined();
    let first = member.handle(1_000, player(MemberState::Playing, 10.0));
    assert_eq!(
        reports(&first),
        vec![Up::Report {
            state: MemberState::Playing,
            position: 10.0,
            duration: Some(7_200.0),
            release_matched: true,
        }]
    );
    assert!(reports(&member.handle(1_500, player(MemberState::Playing, 10.5))).is_empty());
    assert_eq!(
        reports(&member.handle(2_000, player(MemberState::Playing, 11.0))).len(),
        1
    );
}

#[test]
fn paused_member_reports_only_on_change() {
    let mut member = joined();
    member.handle(1_000, player(MemberState::Paused, 10.0));
    assert!(reports(&member.handle(5_000, MemberInput::Tick)).is_empty());
    let out = member.handle(5_100, MemberInput::Release { matched: false });
    assert!(matches!(
        reports(&out)[..],
        [Up::Report {
            release_matched: false,
            ..
        }]
    ));
}

#[test]
fn drift_slows_a_member_ahead() {
    let mut member = watching();
    let out = member.handle(4_000, player(MemberState::Playing, 105.0));
    let speed = speed(&out).expect("a speed correction");
    assert!((speed - 0.95).abs() < 1e-9);
}

#[test]
fn drift_seeks_a_member_far_behind() {
    let mut member = watching();
    let out = member.handle(4_000, player(MemberState::Playing, 101.0));
    assert!(matches!(
        player_commands(&out)[..],
        [MemberOutput::Seek(to)] if (to - 104.15).abs() < 1e-9
    ));
}

#[test]
fn no_drift_correction_without_clock() {
    let mut member = watching();
    // Over 5 s of silence, as when the laptop sleeps, makes the clock stale.
    let out = member.handle(8_000, player(MemberState::Playing, 50.0));
    assert!(player_commands(&out).is_empty());
}

#[test]
fn paused_party_restores_normal_speed() {
    let mut member = watching();
    member.handle(4_000, player(MemberState::Playing, 105.0));
    member.handle(4_100, state(2, false, 104.1, 4_100));
    let out = member.handle(4_200, player(MemberState::Paused, 105.1));
    assert_eq!(speed(&out), Some(1.0));
}

#[test]
fn bye_host_left_emits_ended() {
    let mut member = watching();
    member.handle(4_000, player(MemberState::Playing, 105.0));
    let out = member.handle(
        4_100,
        MemberInput::Down(Down::Bye {
            reason: ByeReason::HostLeft,
        }),
    );
    assert_eq!(
        out,
        vec![
            MemberOutput::SetSpeed(1.0),
            MemberOutput::Ended(ByeReason::HostLeft)
        ]
    );
    assert!(member.handle(4_200, MemberInput::Tick).is_empty());
}
