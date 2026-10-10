#![cfg(windows)]
use panorama_mpv::{
    EndReason, Mpv, MpvError, Player, PlayerEvent, PlayerOptions, SeekMode, TrackKind,
};
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
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

#[test]
fn generated_media_clock_pause_seek_tracks_eof_and_close() {
    let pipe = format!(r"\\.\pipe\panorama-mpv-clock-{}", std::process::id());
    let Some((player, events)) = player(&[
        ("audio-files", "av://lavfi:sine=duration=5"),
        ("input-ipc-server", &pipe),
    ]) else {
        return;
    };
    let (requests, incoming) = mpsc::channel::<()>();
    let (readings, clock) = mpsc::channel();
    let clock_thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let connection = loop {
            match OpenOptions::new().read(true).write(true).open(&pipe) {
                Ok(connection) => break connection,
                Err(error) => {
                    assert!(Instant::now() < deadline, "IPC connection failed: {error}");
                    thread::sleep(Duration::from_millis(10));
                }
            }
        };
        let mut connection = BufReader::new(connection);
        while incoming.recv().is_ok() {
            connection
                .get_mut()
                .write_all(b"{\"command\":[\"get_property\",\"time-pos\"],\"request_id\":1}\n")
                .unwrap();
            loop {
                let mut line = String::new();
                assert_ne!(connection.read_line(&mut line).unwrap(), 0);
                let reply: serde_json::Value = serde_json::from_str(&line).unwrap();
                if reply["request_id"] == 1 {
                    assert_eq!(reply["error"], "success");
                    readings.send(reply["data"].as_f64().unwrap()).unwrap();
                    break;
                }
            }
        }
    });
    player
        .load("av://lavfi:testsrc=duration=5:size=320x240:rate=30")
        .unwrap();
    let mut saw_video = false;
    let mut saw_audio = false;
    let mut first_time = None;
    let mut advanced = false;
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
            }
            PlayerEvent::Error(error) => panic!("generated media failed: {error}"),
            PlayerEvent::EndFile { reason, .. } => panic!("ended before checks: {reason:?}"),
            _ => {}
        }
    }
    player.set_pause(true).unwrap();
    wait(&events, |event| *event == PlayerEvent::Pause(true));
    requests.send(()).unwrap();
    let first = clock.recv_timeout(Duration::from_secs(3)).unwrap();
    let paused_at = Instant::now();
    thread::sleep(Duration::from_millis(500));
    requests.send(()).unwrap();
    let last = clock.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(paused_at.elapsed() >= Duration::from_millis(500));
    assert!((last - first).abs() < 0.01, "paused clock advanced");
    drop(requests);
    clock_thread.join().unwrap();
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
