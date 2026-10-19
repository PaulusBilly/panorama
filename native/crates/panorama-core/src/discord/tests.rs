//! Ports of `tests/unit/discord-presence.test.ts` onto an in-process fake transport.

use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{FutureExt, future::BoxFuture};
use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex},
    runtime::Handle,
};

use super::*;

fn playback() -> DiscordPlayback {
    DiscordPlayback {
        film_id: "tmdb:157336".into(),
        title: "Interstellar".into(),
        year: None,
        director_tmdb_id: None,
        director: Some("Christopher Nolan".into()),
        artwork: Some("https://image.tmdb.org/t/p/w1280/test.jpg".into()),
        time: 60.0,
        duration: 600.0,
        state: PlaybackState::Playing,
    }
}

fn input() -> Value {
    json!({"filmId": "tmdb:157336", "title": "Interstellar", "director": "Christopher Nolan",
        "artwork": "https://image.tmdb.org/t/p/w1280/test.jpg", "time": 60.0, "duration": 600.0,
        "state": "playing"})
}

fn with(mut value: Value, key: &str, field: Value) -> Value {
    value[key] = field;
    value
}

fn parse(value: &Value) -> DiscordPlayback {
    parse_discord_playback(value).unwrap().unwrap()
}

struct Socket {
    io: Option<DuplexStream>,
    received: Vec<u8>,
}

#[derive(Clone, Default)]
struct Fake {
    sockets: Arc<Mutex<Vec<Socket>>>,
    attempts: Arc<Mutex<Vec<String>>>,
    refuse: bool,
}

impl Connector for Fake {
    fn connect(&self, path: &str) -> BoxFuture<'static, io::Result<Box<dyn Io>>> {
        self.attempts.lock().unwrap().push(path.into());
        let result = if self.refuse {
            Err(io::Error::from(io::ErrorKind::NotFound))
        } else {
            let (client, server) = duplex(1 << 16);
            self.sockets.lock().unwrap().push(Socket {
                io: Some(server),
                received: vec![],
            });
            Ok(Box::new(client) as Box<dyn Io>)
        };
        Box::pin(async move { result })
    }
}

impl Fake {
    fn count(&self) -> usize {
        self.sockets.lock().unwrap().len()
    }

    /// Bytes the client wrote so far, and whether it has closed its end.
    fn pump(&self, index: usize) -> bool {
        let mut sockets = self.sockets.lock().unwrap();
        let socket = &mut sockets[index];
        let Some(io) = &mut socket.io else {
            return true;
        };
        let mut buffer = [0u8; 4096];
        loop {
            match io.read(&mut buffer).now_or_never() {
                Some(Ok(0)) => return true,
                Some(Ok(n)) => socket.received.extend_from_slice(&buffer[..n]),
                _ => return false,
            }
        }
    }

    fn frames(&self, index: usize) -> Vec<(u32, Value)> {
        self.pump(index);
        let received = self.sockets.lock().unwrap()[index].received.clone();
        let mut reader = FrameReader::default();
        reader.push(&received);
        let mut frames = vec![];
        while let Some((opcode, body)) = reader.next_frame().unwrap() {
            frames.push((opcode, serde_json::from_slice(&body).unwrap_or(Value::Null)));
        }
        frames
    }

    fn messages(&self, index: usize) -> Vec<Value> {
        self.frames(index)
            .into_iter()
            .map(|(_, value)| value)
            .collect()
    }

    fn closed_by_client(&self, index: usize) -> bool {
        self.pump(index)
    }

    fn emit(&self, index: usize, bytes: &[u8]) {
        let mut sockets = self.sockets.lock().unwrap();
        let io = sockets[index].io.as_mut().unwrap();
        io.write_all(bytes).now_or_never().unwrap().unwrap();
    }

    /// The test's `socket.destroy()`: the peer goes away.
    fn destroy(&self, index: usize) {
        self.pump(index);
        self.sockets.lock().unwrap()[index].io = None;
    }

    async fn ready(&self) -> usize {
        settle().await;
        let index = self.count() - 1;
        let frame = discord_frame(1, &json!({"evt": "READY"}));
        self.emit(index, &frame[..5]);
        settle().await;
        self.emit(index, &frame[5..]);
        settle().await;
        index
    }
}

async fn settle() {
    for _ in 0..40 {
        tokio::time::advance(Duration::ZERO).await;
        tokio::task::yield_now().await;
    }
}

async fn advance(ms: u64) {
    tokio::time::advance(Duration::from_millis(ms)).await;
    settle().await;
}

fn presence(fake: &Fake, file: &std::path::Path) -> DiscordPresence {
    DiscordPresence::new(
        &Handle::current(),
        file,
        vec!["test-socket".into()],
        fake.clone(),
    )
}

#[test]
fn maps_watching_artwork_and_seek_timing_without_advancing_paused_or_buffering_activities() {
    let base = playback();
    let dated = parse(&with(input(), "year", json!("2014")));
    assert_eq!(
        discord_activity(&dated, 1_000_000.0)["details_url"],
        "https://www.themoviedb.org/movie/157336"
    );
    for film_id in ["tt0816692", "tmdb:123/other", "tmdb:0"] {
        let activity = discord_activity(
            &DiscordPlayback {
                film_id: film_id.into(),
                ..base.clone()
            },
            1_000_000.0,
        );
        assert!(activity.get("details_url").is_none());
    }
    let activity = discord_activity(&dated, 1_000_000.0);
    assert_eq!(activity["name"], "in Panorama");
    assert_eq!(activity["state"], "dir. Christopher Nolan");
    assert_eq!(activity["details"], "Interstellar (2014)");
    assert_eq!(activity["status_display_type"], 2);
    let long = DiscordPlayback {
        title: "A".repeat(128),
        ..dated.clone()
    };
    assert_eq!(
        discord_activity(&long, 1_000_000.0)["details"],
        format!("{} (2014)", "A".repeat(121))
    );
    assert!(parse_discord_playback(&with(input(), "year", json!(2014))).is_err());
    let linked = parse(&with(input(), "directorTmdbId", json!(525)));
    assert_eq!(
        discord_activity(&linked, 1_000_000.0)["state_url"],
        "https://www.themoviedb.org/person/525"
    );
    let no_director = DiscordPlayback {
        director: None,
        ..linked.clone()
    };
    assert!(
        discord_activity(&no_director, 1_000_000.0)
            .get("state_url")
            .is_none()
    );
    assert!(
        discord_activity(&base, 1_000_000.0)
            .get("state_url")
            .is_none()
    );
    for id in [json!(-1), json!(0), json!(1.5), json!("525")] {
        assert!(parse_discord_playback(&with(input(), "directorTmdbId", id)).is_err());
    }
    for state in [
        PlaybackState::Playing,
        PlaybackState::Paused,
        PlaybackState::Buffering,
    ] {
        let activity = discord_activity(
            &DiscordPlayback {
                state,
                ..base.clone()
            },
            1_000_000.0,
        );
        assert_eq!(activity["state"], "dir. Christopher Nolan");
    }
    assert!(parse_discord_playback(&with(input(), "director", json!(42))).is_err());
    let activity = discord_activity(&base, 1_000_000.0);
    assert_eq!(activity["type"], 3);
    assert_eq!(activity["details"], "Interstellar");
    assert_eq!(activity["status_display_type"], 2);
    assert_eq!(activity["timestamps"], json!({"start": 940, "end": 1540}));
    assert_eq!(
        activity["assets"]["large_image"],
        base.artwork.clone().unwrap()
    );
    let later = DiscordPlayback {
        time: 120.0,
        ..base.clone()
    };
    assert_eq!(
        discord_activity(&later, 1_000_000.0)["timestamps"]["start"],
        880
    );
    for state in [PlaybackState::Paused, PlaybackState::Buffering] {
        let activity = discord_activity(
            &DiscordPlayback {
                state,
                ..base.clone()
            },
            1_000_000.0,
        );
        assert!(activity.get("timestamps").is_none());
    }
    let bare = DiscordPlayback {
        artwork: None,
        ..base.clone()
    };
    assert!(discord_activity(&bare, 1_000_000.0).get("assets").is_none());
    assert_eq!(
        discord_artwork(&[
            Some("http://image.tmdb.org/t/p/test.jpg"),
            base.artwork.as_deref()
        ]),
        base.artwork
    );
    assert_eq!(
        discord_artwork(&[
            Some("https://image.tmdb.org.evil.test/t/p/a.jpg"),
            Some("https://user@image.tmdb.org/t/p/a.jpg"),
        ]),
        None
    );
    assert_eq!(parse(&input()), base);
    // JSON cannot carry NaN; a null time exercises the same "not a finite number" branch.
    for bad in [
        with(input(), "time", Value::Null),
        with(input(), "duration", json!(-1)),
        with(input(), "artwork", json!("https://evil.test/a")),
        with(input(), "secret", json!("x")),
        with(input(), "state", json!("ended")),
    ] {
        assert!(parse_discord_playback(&bad).is_err());
    }
    assert_eq!(parse_discord_playback(&Value::Null), Ok(None));
}

#[tokio::test(start_paused = true)]
async fn defaults_off_persists_opt_in_handles_split_frames_coalesces_updates_and_clears_immediately()
 {
    let directory = tempdir().unwrap();
    let file = directory.path().join("settings.json");
    let fake = Fake::default();
    let presence = presence(&fake, &file);
    presence.update(playback());
    settle().await;
    assert_eq!(fake.count(), 0);
    assert_eq!(
        presence.settings(),
        DiscordSettings {
            enabled: false,
            available: true
        }
    );
    presence.set_enabled(true).unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved, json!({"enabled": true}));
    assert!(self::presence(&Fake::default(), &file).settings().enabled);
    let socket = fake.ready().await;
    let messages = fake.messages(socket);
    assert_eq!(
        messages[0],
        json!({"v": 1, "client_id": "1549772711264395274"})
    );
    assert_eq!(messages[1]["args"]["activity"]["details"], "Interstellar");
    presence.update(DiscordPlayback {
        time: 200.0,
        ..playback()
    });
    presence.update(DiscordPlayback {
        state: PlaybackState::Paused,
        time: 210.0,
        ..playback()
    });
    advance(4999).await;
    assert_eq!(fake.messages(socket).len(), 2);
    advance(1).await;
    let messages = fake.messages(socket);
    assert_eq!(
        messages[2]["args"]["activity"]["state"],
        "dir. Christopher Nolan"
    );
    assert!(messages[2]["args"]["activity"].get("timestamps").is_none());
    presence.set_enabled(false).unwrap();
    settle().await;
    assert!(fake.messages(socket).last().unwrap()["args"]["activity"].is_null());
    assert!(fake.closed_by_client(socket));
    advance(30000).await;
    assert_eq!(fake.count(), 1);
}

#[tokio::test(start_paused = true)]
async fn retries_disconnected_clients_after_15_seconds_and_cancels_retries_on_clear() {
    let directory = tempdir().unwrap();
    let fake = Fake::default();
    let presence = presence(&fake, &directory.path().join("settings.json"));
    presence.set_enabled(true).unwrap();
    presence.update(playback());
    settle().await;
    fake.destroy(0);
    settle().await;
    advance(14999).await;
    assert_eq!(fake.count(), 1);
    advance(1).await;
    let socket = fake.ready().await;
    assert_eq!(fake.count(), 2);
    let pings = [
        discord_frame(3, &json!({"ping": 1})),
        discord_frame(3, &json!({"ping": 2})),
    ]
    .concat();
    fake.emit(socket, &pings);
    settle().await;
    let frames = fake.frames(socket);
    assert_eq!(frames.iter().filter(|(opcode, _)| *opcode == 4).count(), 2);
    assert_eq!(frames.last().unwrap().0, 4);
    fake.destroy(socket);
    settle().await;
    presence.clear();
    advance(30000).await;
    assert_eq!(fake.count(), 2);
}

#[tokio::test(start_paused = true)]
async fn rejects_oversized_frames_and_discovers_platform_socket_paths() {
    let directory = tempdir().unwrap();
    let fake = Fake::default();
    let presence = presence(&fake, &directory.path().join("settings.json"));
    presence.set_enabled(true).unwrap();
    presence.update(playback());
    let socket = fake.ready().await;
    let mut header = [0u8; 8];
    header[4..].copy_from_slice(&65537u32.to_le_bytes());
    fake.emit(socket, &header);
    settle().await;
    assert!(fake.closed_by_client(socket));
    assert_eq!(
        discord_paths(Platform::Windows, |_| None)[0],
        r"\\?\pipe\discord-ipc-0"
    );
    let env = HashMap::from([("TMPDIR", "/test"), ("NODE_ENV", "test")]);
    let paths = discord_paths(Platform::Unix, |name| env.get(name).map(|v| v.to_string()));
    assert_eq!(paths[9], "/test/discord-ipc-9");
    assert_eq!(
        discord_paths(Platform::Unix, |_| None)[0],
        "/tmp/discord-ipc-0"
    );
}

#[tokio::test(start_paused = true)]
async fn missing_discord_is_silent_and_walks_every_path_before_a_15_second_retry() {
    let directory = tempdir().unwrap();
    let fake = Fake {
        refuse: true,
        ..Fake::default()
    };
    let presence = DiscordPresence::new(
        &Handle::current(),
        directory.path().join("settings.json"),
        vec!["a".into(), "b".into()],
        fake.clone(),
    );
    presence.set_enabled(true).unwrap();
    presence.update(playback());
    settle().await;
    assert_eq!(*fake.attempts.lock().unwrap(), ["a", "b"]);
    advance(15000).await;
    assert_eq!(fake.attempts.lock().unwrap().len(), 4);
    presence.clear();
    advance(60000).await;
    assert_eq!(fake.attempts.lock().unwrap().len(), 4);
}

#[tokio::test(start_paused = true)]
async fn untrusted_updates_are_validated_and_null_clears_the_activity() {
    let directory = tempdir().unwrap();
    let fake = Fake::default();
    let presence = presence(&fake, &directory.path().join("settings.json"));
    presence.set_enabled(true).unwrap();
    assert!(
        presence
            .update_value(&with(input(), "secret", json!(1)))
            .is_err()
    );
    settle().await;
    assert_eq!(fake.count(), 0);
    presence.update_value(&input()).unwrap();
    let socket = fake.ready().await;
    presence.update_value(&Value::Null).unwrap();
    settle().await;
    assert!(fake.messages(socket).last().unwrap()["args"]["activity"].is_null());
    assert!(fake.closed_by_client(socket));
}
