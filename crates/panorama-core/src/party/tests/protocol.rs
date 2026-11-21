use super::*;

fn ups() -> Vec<Up> {
    vec![
        Up::Hello {
            name: "Sam".into(),
            version: PROTOCOL_VERSION,
            secret: secret(),
        },
        Up::Ping {
            id: 7,
            sent_ms: 1_234,
        },
        Up::Report {
            state: MemberState::Buffering,
            position: 61.5,
            duration: Some(7_200.0),
            release_matched: false,
        },
        Up::Request {
            action: Action::Play,
        },
        Up::Request {
            action: Action::Pause,
        },
        Up::Request {
            action: Action::Seek { to: 90.25 },
        },
    ]
}

fn downs() -> Vec<Down> {
    vec![
        Down::Welcome { member_id: 3 },
        Down::Pong {
            id: 7,
            sent_ms: 1_234,
            host_ms: 99_000,
        },
        Down::Load {
            media: media(),
            revision: 4,
        },
        Down::State {
            revision: 5,
            playback: PlaybackState {
                playing: true,
                anchor_position: 100.0,
                anchor_host_ms: 99_000,
            },
            anyone_controls: true,
            lobby: true,
            waiting_for: Some(2),
            tracks: Some(english_subtitles()),
        },
        Down::Roster {
            members: vec![MemberView {
                member_id: 0,
                name: "Alex".into(),
                state: MemberState::Playing,
                late: false,
                release_matched: true,
                is_host: true,
            }],
        },
        Down::Notice(Notice {
            kind: NoticeKind::Paused,
            by: "Alex".into(),
            at: Some(724.0),
        }),
        Down::Bye {
            reason: ByeReason::BadSecret,
        },
    ]
}

#[test]
fn round_trips_every_variant() {
    for msg in ups() {
        assert_eq!(decode_up(&encode(&msg)), Ok(msg));
    }
    for msg in downs() {
        assert_eq!(decode_down(&encode(&msg)), Ok(msg));
    }
}

#[test]
fn encoding_is_one_line_tagged_by_t() {
    let line = encode(&Up::Request {
        action: Action::Seek { to: 5.0 },
    });
    assert_eq!(line, r#"{"t":"request","action":{"t":"seek","to":5.0}}"#);
}

#[test]
fn rejects_line_over_64k() {
    let line = format!(
        r#"{{"t":"ping","id":1,"sent_ms":1,"pad":"{}"}}"#,
        "x".repeat(MAX_LINE)
    );
    assert_eq!(decode_up(&line), Err(ProtocolError::TooLong));
    assert_eq!(decode_down(&line), Err(ProtocolError::TooLong));
}

#[test]
fn rejects_unknown_fields() {
    let hello = format!(r#"{{"t":"hello","name":"Sam","version":1,"secret":"{SECRET}","x":1}}"#);
    assert_eq!(decode_up(&hello), Err(ProtocolError::Malformed));
    let notice = r#"{"t":"notice","kind":"paused","by":"Alex","at":null,"url":"https://x"}"#;
    assert_eq!(decode_down(notice), Err(ProtocolError::Malformed));
}

#[test]
fn rejects_non_finite_or_out_of_range_position() {
    let report = |position: &str| {
        decode_up(&format!(
            r#"{{"t":"report","state":"playing","position":{position},"duration":null,"release_matched":true}}"#
        ))
    };
    assert_eq!(report("-1"), Err(ProtocolError::Invalid("position")));
    assert_eq!(report("86401"), Err(ProtocolError::Invalid("position")));
    assert!(report("1e999").is_err());
    assert!(report("86400").is_ok());
    let seek = r#"{"t":"request","action":{"t":"seek","to":-0.5}}"#;
    assert_eq!(decode_up(seek), Err(ProtocolError::Invalid("position")));
}

#[test]
fn trims_and_caps_name() {
    let hello = |name: &str| {
        decode_up(&format!(
            r#"{{"t":"hello","name":"{name}","version":1,"secret":"{SECRET}"}}"#
        ))
    };
    assert_eq!(hello("  "), Err(ProtocolError::Invalid("name")));
    assert_eq!(hello(&"n".repeat(40)), Err(ProtocolError::Invalid("name")));
    assert!(matches!(hello("  Sam "), Ok(Up::Hello { name, .. }) if name == "Sam"));
}

#[test]
fn secret_must_be_32_lowercase_hex() {
    for secret in ["0123", &SECRET.to_uppercase(), &format!("{SECRET}0")] {
        let line = format!(r#"{{"t":"hello","name":"Sam","version":1,"secret":"{secret}"}}"#);
        assert_eq!(decode_up(&line), Err(ProtocolError::Malformed));
    }
}

#[test]
fn load_rejects_bad_release_fields() {
    let load = |edit: fn(&mut PartyMedia)| {
        let mut media = media();
        edit(&mut media);
        decode_down(&encode(&Down::Load { media, revision: 1 }))
    };
    let invalid = |field| Err(ProtocolError::Invalid(field));
    assert_eq!(
        load(|m| m.release.torrent.as_mut().unwrap().info_hash = "A".repeat(40)),
        invalid("info_hash")
    );
    assert_eq!(load(|m| m.film_id = "a/b".into()), invalid("film_id"));
    assert_eq!(load(|m| m.video_id = String::new()), invalid("video_id"));
    assert_eq!(load(|m| m.title = "t".repeat(201)), invalid("title"));
    assert_eq!(
        load(|m| m.release.addon = "id#salt".into()),
        invalid("addon")
    );
    assert_eq!(
        load(|m| m.release.label = "l".repeat(5001)),
        invalid("label")
    );
    assert_eq!(
        load(|m| m.release.binge_group = Some("b".repeat(201))),
        invalid("binge_group")
    );
    assert!(load(|m| m.release.torrent = None).is_ok());
}

#[test]
fn roster_is_capped_at_party_size() {
    let member = MemberView {
        member_id: 1,
        name: "Sam".into(),
        state: MemberState::Ready,
        late: false,
        release_matched: true,
        is_host: false,
    };
    let roster = Down::Roster {
        members: vec![member; MAX_MEMBERS + 1],
    };
    assert_eq!(
        decode_down(&encode(&roster)),
        Err(ProtocolError::Invalid("roster"))
    );
}

#[test]
fn rejects_times_outside_a_safe_range() {
    let pong = |sent_ms: i64, host_ms: i64| {
        decode_down(&encode(&Down::Pong {
            id: 1,
            sent_ms,
            host_ms,
        }))
    };
    let invalid = Err(ProtocolError::Invalid("time"));
    assert_eq!(pong(0, i64::MAX), invalid);
    assert_eq!(pong(i64::MIN, 0), invalid);
    assert_eq!(pong(0, -1), invalid);
    assert!(pong(0, 1 << 53).is_ok());
    let state = |anchor_host_ms: i64| {
        decode_down(&encode(&Down::State {
            revision: 1,
            playback: PlaybackState {
                playing: true,
                anchor_position: 0.0,
                anchor_host_ms,
            },
            anyone_controls: false,
            lobby: false,
            waiting_for: None,
            tracks: None,
        }))
    };
    assert_eq!(state(i64::MIN), invalid);
    assert_eq!(state((1 << 53) + 1), invalid);
    let ping = encode(&Up::Ping {
        id: 1,
        sent_ms: i64::MAX,
    });
    assert_eq!(decode_up(&ping), Err(ProtocolError::Invalid("time")));
}

#[test]
fn names_refuse_invisible_format_characters() {
    let hello = |name: &str| {
        decode_up(&encode(&Up::Hello {
            name: name.into(),
            version: PROTOCOL_VERSION,
            secret: secret(),
        }))
    };
    for name in ["\u{202E}xelA", "Al\u{200B}ex", "\u{2066}Sam", "Sam\u{FEFF}"] {
        assert_eq!(hello(name), Err(ProtocolError::Invalid("name")), "{name:?}");
    }
    assert!(hello("Zoë 🍿").is_ok());
}

fn state_with(tracks: TrackChoice) -> Result<Down, ProtocolError> {
    decode_down(&encode(&Down::State {
        revision: 1,
        playback: PlaybackState {
            playing: false,
            anchor_position: 0.0,
            anchor_host_ms: 0,
        },
        anyone_controls: false,
        lobby: false,
        waiting_for: None,
        tracks: Some(tracks),
    }))
}

#[test]
fn a_track_choice_round_trips() {
    let tracks = TrackChoice {
        audio: Some(TrackKey {
            language: Some("ja".into()),
            title: None,
            codec: Some("eac3".into()),
            external: false,
        }),
        ..english_subtitles()
    };
    let Ok(Down::State {
        tracks: decoded, ..
    }) = state_with(tracks.clone())
    else {
        panic!("a valid track choice decodes");
    };
    assert_eq!(decoded, Some(tracks));
    assert!(state_with(TrackChoice::default()).is_ok());
}

#[test]
fn an_oversized_track_field_is_rejected() {
    let with = |change: fn(&mut TrackKey)| {
        let mut tracks = english_subtitles();
        change(tracks.subtitle.as_mut().unwrap());
        state_with(tracks)
    };
    assert_eq!(
        with(|key| key.language = Some("e".repeat(17))),
        Err(ProtocolError::Invalid("language"))
    );
    assert_eq!(
        with(|key| key.title = Some("t".repeat(201))),
        Err(ProtocolError::Invalid("track_title"))
    );
    assert_eq!(
        with(|key| key.codec = Some("c".repeat(33))),
        Err(ProtocolError::Invalid("codec"))
    );
    assert_eq!(
        with(|key| key.title = Some("Eng\u{202E}lish".into())),
        Err(ProtocolError::Invalid("track_title"))
    );
    assert!(with(|key| key.title = Some("t".repeat(200))).is_ok());
}

#[test]
fn a_track_title_never_prints() {
    let printed = format!("{:?}", english_subtitles());
    assert!(!printed.contains("SDH"), "{printed}");
    assert!(printed.contains("\"en\""), "{printed}");
}
