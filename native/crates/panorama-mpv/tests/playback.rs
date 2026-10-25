#![cfg(any(windows, target_os = "macos"))]
use panorama_mpv::{
    EndReason, Mpv, MpvError, Player, PlayerEvent, PlayerOptions, SeekMode, TrackKind,
};
use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

fn player(extra: &[(&str, &str)]) -> Option<(Player, Receiver<PlayerEvent>)> {
    let Some(directory) = std::env::var_os("PANORAMA_LIBMPV_DIR") else {
        eprintln!("SKIPPED libmpv integration test: PANORAMA_LIBMPV_DIR is unset");
        return None;
    };
    eprintln!("Running libmpv integration test with the configured runtime");
    let library = Mpv::load_library(Some(&PathBuf::from(directory))).unwrap();
    let mut options = PlayerOptions {
        wid: None,
        extra: vec![
            ("vo".into(), "null".into()),
            ("ao".into(), "null".into()),
            ("keep-open".into(), "no".into()),
            ("http-proxy".into(), "".into()),
        ],
    };
    options
        .extra
        .extend(extra.iter().map(|&(k, v)| (k.into(), v.into())));
    let mut player = library.create(options).unwrap();
    let events = player.events().unwrap();
    wait(&events, |event| *event == PlayerEvent::Ready);
    Some((player, events))
}

fn wait(events: &Receiver<PlayerEvent>, predicate: impl Fn(&PlayerEvent) -> bool) -> PlayerEvent {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let event = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        assert!(
            !matches!(event, PlayerEvent::Error(_)),
            "runtime error: {event:?}"
        );
        if predicate(&event) {
            return event;
        }
    }
}

fn close(player: Player) {
    let started = Instant::now();
    let completion = player.close();
    assert!(started.elapsed() < Duration::from_millis(200));
    assert_eq!(
        completion.recv_timeout(Duration::from_secs(3)).unwrap(),
        Ok(())
    );
}

/// Five seconds of 30 fps Y4M video and an 8 kHz WAV tone; every libmpv build demuxes
/// both, unlike `av://lavfi`, which needs libavdevice.
fn media() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let (width, height, frames) = (64, 48, 150);
    let mut video = format!("YUV4MPEG2 W{width} H{height} F30:1 Ip A1:1 C420jpeg\n").into_bytes();
    for frame in 0..frames {
        video.extend_from_slice(b"FRAME\n");
        video.extend(std::iter::repeat_n(
            (frame * 255 / frames) as u8,
            width * height,
        ));
        video.extend(std::iter::repeat_n(128, width * height / 2));
    }
    let samples: Vec<u8> = (0..8000 * 5)
        .flat_map(|i| {
            (((i as f64 * 440.0 / 8000.0 * std::f64::consts::TAU).sin() * 8000.0) as i16)
                .to_le_bytes()
        })
        .collect();
    let mut audio = b"RIFF".to_vec();
    audio.extend((36 + samples.len() as u32).to_le_bytes());
    audio.extend(b"WAVEfmt ");
    for field in [16u32, 1 | 1 << 16, 8000, 16000, 2 | 16 << 16] {
        audio.extend(field.to_le_bytes());
    }
    audio.extend(b"data");
    audio.extend((samples.len() as u32).to_le_bytes());
    audio.extend(samples);
    let (video_path, audio_path) = (
        directory.path().join("clip.y4m"),
        directory.path().join("tone.wav"),
    );
    std::fs::write(&video_path, video).unwrap();
    std::fs::write(&audio_path, audio).unwrap();
    (directory, video_path, audio_path)
}

#[test]
fn generated_media_clock_pause_seek_tracks_eof_and_close() {
    let (_media, video, audio) = media();
    let Some((player, events)) = player(&[("audio-files", audio.to_str().unwrap())]) else {
        return;
    };
    player.load(video.to_str().unwrap()).unwrap();
    let mut saw_video = false;
    let mut saw_audio = false;
    let mut first_time = None;
    let mut advanced = false;
    let mut last = 0.0;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !(saw_video && saw_audio && advanced) {
        let event = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        match event {
            PlayerEvent::TrackList(tracks) => {
                saw_video |= tracks.iter().any(|track| track.kind == TrackKind::Video);
                saw_audio |= tracks.iter().any(|track| track.kind == TrackKind::Audio);
            }
            PlayerEvent::TimePos(Some(time)) => {
                let first = *first_time.get_or_insert(time);
                advanced |= time - first >= 0.3;
                last = time;
            }
            PlayerEvent::Error(error) => panic!("generated media failed: {error}"),
            PlayerEvent::EndFile { reason, .. } => panic!("ended before checks: {reason:?}"),
            _ => {}
        }
    }
    player.set_pause(true).unwrap();
    let next = || {
        events
            .recv_timeout(Duration::from_secs(15))
            .map(|event| {
                assert!(!matches!(event, PlayerEvent::Error(_)), "{event:?}");
                event
            })
            .unwrap()
    };
    loop {
        match next() {
            PlayerEvent::TimePos(Some(time)) => last = time,
            PlayerEvent::Pause(true) => break,
            _ => {}
        }
    }
    thread::sleep(Duration::from_millis(500));
    player.set_pause(false).unwrap();
    while next() != PlayerEvent::Pause(false) {}
    let resumed = loop {
        if let PlayerEvent::TimePos(Some(time)) = next() {
            break time;
        }
    };
    assert!(
        resumed - last < 0.1,
        "paused clock advanced: {last} -> {resumed}"
    );
    player.set_pause(true).unwrap();
    wait(&events, |event| *event == PlayerEvent::Pause(true));
    player.seek(2.0, SeekMode::Absolute).unwrap();
    wait(
        &events,
        |event| matches!(event, PlayerEvent::TimePos(Some(t)) if (t - 2.0).abs() < 0.3),
    );
    player.seek(-0.5, SeekMode::Relative).unwrap();
    wait(
        &events,
        |event| matches!(event, PlayerEvent::TimePos(Some(t)) if (t - 1.5).abs() < 0.3),
    );
    player.set_pause(false).unwrap();
    wait(&events, |event| *event == PlayerEvent::Pause(false));
    wait(&events, |event| {
        matches!(
            event,
            PlayerEvent::EndFile {
                reason: EndReason::Eof,
                ..
            }
        )
    });
    close(player);
    wait(&events, |event| *event == PlayerEvent::Shutdown);
}

#[test]
fn unreachable_url_error_is_redacted_and_close_is_prompt() {
    let Some((player, events)) = player(&[]) else {
        return;
    };
    let url = "http://127.0.0.1:1/x?session=private-token";
    player.load(url).unwrap();
    let event = wait(&events, |event| {
        matches!(
            event,
            PlayerEvent::EndFile {
                reason: EndReason::Error(_),
                ..
            }
        )
    });
    let PlayerEvent::EndFile {
        reason: EndReason::Error(error),
        ..
    } = event
    else {
        unreachable!()
    };
    for message in [error.to_string(), format!("{error:?}")] {
        assert!(!message.contains(url));
        assert!(!message.contains("private-token"));
        assert!(!message.contains("127.0.0.1"));
    }
    assert!(matches!(error, MpvError::Loading | MpvError::Demuxer));
    close(player);
    wait(&events, |event| *event == PlayerEvent::Shutdown);
}

#[test]
fn close_while_network_load_is_pending_is_bounded() {
    let Some((player, events)) = player(&[]) else {
        return;
    };
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/x?session=private-token",
        server.local_addr().unwrap()
    );
    server.set_nonblocking(true).unwrap();
    let (received, request) = mpsc::channel();
    let (release, held) = mpsc::channel::<()>();
    let server_thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        let connection = loop {
            match server.accept() {
                Ok((connection, _)) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "mpv never connected");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        connection.set_nonblocking(false).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut connection = BufReader::new(connection);
        let mut line = String::new();
        connection.read_line(&mut line).unwrap();
        assert!(line.starts_with("GET /x?session=private-token HTTP/"));
        loop {
            line.clear();
            assert_ne!(connection.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" {
                break;
            }
        }
        received.send(()).unwrap();
        let _ = held.recv_timeout(Duration::from_secs(15));
        drop(connection);
    });
    player.load(&url).unwrap();
    request.recv_timeout(Duration::from_secs(15)).unwrap();
    let started = Instant::now();
    let completion = player.close();
    assert!(started.elapsed() < Duration::from_millis(200));
    let deadline = Duration::from_millis(2250);
    assert_eq!(completion.recv_timeout(deadline).unwrap(), Ok(()));
    assert!(started.elapsed() < deadline);
    loop {
        let event = events.recv_timeout(Duration::from_secs(3)).unwrap();
        let mut messages = vec![format!("{event:?}")];
        match &event {
            PlayerEvent::Error(error)
            | PlayerEvent::EndFile {
                reason: EndReason::Error(error),
                ..
            } => messages.push(error.to_string()),
            _ => {}
        }
        for message in messages {
            assert!(!message.contains(&url));
            assert!(!message.contains("private-token"));
            assert!(!message.contains("127.0.0.1"));
        }
        if event == PlayerEvent::Shutdown {
            break;
        }
    }
    drop(release);
    server_thread.join().unwrap();
}

#[test]
fn extra_options_cannot_reach_wid_or_load_configuration() {
    let Some(directory) = std::env::var_os("PANORAMA_LIBMPV_DIR") else {
        eprintln!("SKIPPED libmpv integration test: PANORAMA_LIBMPV_DIR is unset");
        return;
    };
    let library = Mpv::load_library(Some(&PathBuf::from(directory))).unwrap();
    for name in ["wid", "include", "config", "config-dir", "profile"] {
        let options = PlayerOptions {
            wid: None,
            extra: vec![(name.into(), "1".into())],
        };
        assert_eq!(
            library.create(options).err(),
            Some(MpvError::InvalidArgument),
            "{name}"
        );
    }
}

#[test]
fn load_at_opens_the_file_at_its_position() {
    let (_media, video, _) = media();
    let Some((player, events)) = player(&[]) else {
        return;
    };
    player.load_at(video.to_str().unwrap(), Some(3.0)).unwrap();
    let PlayerEvent::TimePos(Some(first)) = wait(&events, |event| {
        matches!(event, PlayerEvent::TimePos(Some(_)))
    }) else {
        unreachable!()
    };
    assert!(
        first >= 2.9,
        "playback began at {first}s, not the resume point"
    );
    close(player);
}
