use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    runtime::Handle,
    sync::mpsc,
    time::{Instant, sleep_until, timeout},
};

use super::ipc::{FrameReader, raw_frame};
use super::{
    APPLICATION_ID, Connector, DiscordPlayback, InvalidPlayback, Io, PlaybackState,
    discord_activity, discord_frame, parse_discord_playback,
};

/// Minimum spacing between activity updates.
const THROTTLE: Duration = Duration::from_secs(5);
/// Wait before retrying after every path failed or a connection dropped.
const RETRY: Duration = Duration::from_secs(15);
/// Time Discord has to answer the handshake with `READY`.
const HANDSHAKE: Duration = Duration::from_secs(5);
/// Bound on a single write or connect attempt.
const IO_LIMIT: Duration = Duration::from_secs(5);

/// The persisted opt-in and whether presence can run at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiscordSettings {
    /// The user turned presence on.
    pub enabled: bool,
    /// The application ID is well formed (17 to 20 digits).
    pub available: bool,
}

fn available() -> bool {
    (17..=20).contains(&APPLICATION_ID.len()) && APPLICATION_ID.bytes().all(|b| b.is_ascii_digit())
}

enum Command {
    Enable(bool),
    Update(Option<DiscordPlayback>),
}

/// Handle to the presence task. Methods only queue work and never block, so any
/// thread may call them. Dropping the handle clears the activity and disconnects.
pub struct DiscordPresence {
    commands: mpsc::UnboundedSender<Command>,
    file: PathBuf,
    enabled: Arc<AtomicBool>,
}

impl DiscordPresence {
    /// Starts the presence task on `runtime`. `file` stores the opt-in (default off),
    /// `paths` are the candidate IPC paths in order (see [`super::discord_paths`]).
    pub fn new(
        runtime: &Handle,
        file: impl Into<PathBuf>,
        paths: Vec<String>,
        connector: impl Connector,
    ) -> Self {
        let file = file.into();
        let enabled = fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .is_some_and(|value| value["enabled"] == true);
        let flag = Arc::new(AtomicBool::new(enabled));
        let (commands, receiver) = mpsc::unbounded_channel();
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
        runtime.spawn(
            Actor {
                commands: receiver,
                connector: Arc::new(connector),
                paths,
                enabled,
                playback: None,
                connection: None,
                timer: None,
                last_sent: None,
                clock: (started, Instant::now()),
            }
            .run(),
        );
        Self {
            commands,
            file,
            enabled: flag,
        }
    }

    /// The current opt-in and availability.
    pub fn settings(&self) -> DiscordSettings {
        DiscordSettings {
            enabled: self.enabled.load(Ordering::Acquire),
            available: available(),
        }
    }

    /// Persists the opt-in (atomically, owner-only) and applies it. Turning it off
    /// clears the activity and disconnects even if persisting fails.
    pub fn set_enabled(&self, value: bool) -> io::Result<DiscordSettings> {
        if value && !available() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid Discord setting",
            ));
        }
        if !value {
            self.enabled.store(false, Ordering::Release);
            let _ = self.commands.send(Command::Enable(false));
        }
        persist(&self.file, value)?;
        if value {
            self.enabled.store(true, Ordering::Release);
            let _ = self.commands.send(Command::Enable(true));
        }
        Ok(self.settings())
    }

    /// Reports the current playback; a later report replaces an unsent earlier one.
    pub fn update(&self, playback: DiscordPlayback) {
        let _ = self.commands.send(Command::Update(Some(playback)));
    }

    /// Validates an untrusted JSON playback (`null` clears) and reports it.
    pub fn update_value(&self, value: &Value) -> Result<(), InvalidPlayback> {
        let _ = self
            .commands
            .send(Command::Update(parse_discord_playback(value)?));
        Ok(())
    }

    /// Clears the activity and disconnects.
    pub fn clear(&self) {
        let _ = self.commands.send(Command::Update(None));
    }
}

fn persist(file: &Path, enabled: bool) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| io::Error::other("no randomness"))?;
    let name = format!(
        "{}.{}.tmp",
        file.display(),
        nonce.map(|b| format!("{b:02x}")).concat()
    );
    let temporary = PathBuf::from(name);
    let write = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        io::Write::write_all(
            &mut options.open(&temporary)?,
            json!({"enabled": enabled}).to_string().as_bytes(),
        )?;
        fs::rename(&temporary, file)
    })();
    let _ = fs::remove_file(&temporary);
    write
}

fn nonce() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return "0".into();
    }
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[derive(Clone, Copy)]
enum Timer {
    /// Throttled `SET_ACTIVITY`.
    Send,
    /// Reconnect after every path failed.
    Retry,
}

struct Connection {
    io: Box<dyn Io>,
    reader: FrameReader,
    index: usize,
    ready: bool,
    /// Handshake deadline, cleared by `READY`.
    deadline: Option<Instant>,
}

struct Actor {
    commands: mpsc::UnboundedReceiver<Command>,
    connector: Arc<dyn Connector>,
    paths: Vec<String>,
    enabled: bool,
    /// Latest report and when it arrived.
    playback: Option<(DiscordPlayback, Instant)>,
    connection: Option<Connection>,
    timer: Option<(Instant, Timer)>,
    last_sent: Option<Instant>,
    /// Wall-clock milliseconds at an `Instant`, so timestamps follow the (pausable) tokio clock.
    clock: (f64, Instant),
}

enum Event {
    Command(Option<Command>),
    Read(io::Result<usize>),
    Timer(Timer),
    Handshake,
}

impl Actor {
    async fn run(mut self) {
        let mut chunk = [0u8; 4096];
        loop {
            let timer = self.timer;
            let deadline = self.connection.as_ref().and_then(|c| c.deadline);
            let event = {
                let Self {
                    commands,
                    connection,
                    ..
                } = &mut self;
                tokio::select! {
                    biased;
                    command = commands.recv() => Event::Command(command),
                    read = async {
                        match connection {
                            Some(c) => c.io.read(&mut chunk).await,
                            None => std::future::pending().await,
                        }
                    } => Event::Read(read),
                    kind = async {
                        match timer {
                            Some((at, kind)) => { sleep_until(at).await; kind }
                            None => std::future::pending().await,
                        }
                    } => Event::Timer(kind),
                    () = async {
                        match deadline {
                            Some(at) => sleep_until(at).await,
                            None => std::future::pending().await,
                        }
                    } => Event::Handshake,
                }
            };
            let alive = match event {
                Event::Command(Some(command)) => {
                    self.command(command).await;
                    true
                }
                Event::Command(None) => {
                    self.playback = None;
                    self.disconnect().await;
                    false
                }
                Event::Read(Ok(n)) if n > 0 => {
                    if let Some(c) = &mut self.connection {
                        c.reader.push(&chunk[..n]);
                    }
                    if self.drain_frames().await.is_err() {
                        self.closed().await;
                    }
                    true
                }
                Event::Read(_) | Event::Handshake => {
                    self.closed().await;
                    true
                }
                Event::Timer(kind) => {
                    self.timer = None;
                    match kind {
                        Timer::Send => {
                            if self.send_activity().await.is_err() {
                                self.closed().await;
                            }
                        }
                        Timer::Retry => self.schedule().await,
                    }
                    true
                }
            };
            if !alive {
                return;
            }
        }
    }

    async fn command(&mut self, command: Command) {
        match command {
            Command::Enable(value) => {
                if !value {
                    self.enabled = false;
                    self.disconnect().await;
                }
                self.enabled = value;
            }
            Command::Update(playback) => {
                let present = playback.is_some();
                self.playback = playback.map(|p| (p, Instant::now()));
                if !present {
                    self.disconnect().await;
                    return;
                }
            }
        }
        self.schedule().await;
    }

    fn now_ms(&self) -> f64 {
        self.clock.0 + self.clock.1.elapsed().as_secs_f64() * 1000.0
    }

    async fn schedule(&mut self) {
        if !self.enabled || self.playback.is_none() || !available() || self.timer.is_some() {
            return;
        }
        match &self.connection {
            None => self.connect(0).await,
            Some(c) if !c.ready => {}
            Some(_) => {
                let at = self
                    .last_sent
                    .map_or_else(Instant::now, |sent| sent + THROTTLE);
                self.timer = Some((at.max(Instant::now()), Timer::Send));
            }
        }
    }

    /// Tries `paths[index..]` until one opens; if none does, retries in 15 s.
    async fn connect(&mut self, mut index: usize) {
        loop {
            if !self.enabled || self.playback.is_none() {
                return;
            }
            let Some(path) = self.paths.get(index) else {
                self.timer = Some((Instant::now() + RETRY, Timer::Retry));
                return;
            };
            let opened = timeout(IO_LIMIT, self.connector.connect(path)).await;
            if let Ok(Ok(io)) = opened {
                self.connection = Some(Connection {
                    io,
                    reader: FrameReader::default(),
                    index,
                    ready: false,
                    deadline: Some(Instant::now() + HANDSHAKE),
                });
                let hello = json!({"v": 1, "client_id": APPLICATION_ID});
                if self.write(&discord_frame(0, &hello)).await.is_ok() {
                    return;
                }
                self.connection = None;
            }
            index += 1;
        }
    }

    /// The socket closed or failed: move on to the next path (never connected) or
    /// wait out the retry delay (was connected).
    async fn closed(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        self.timer = None;
        let next = if connection.ready {
            self.paths.len()
        } else {
            connection.index + 1
        };
        self.connect(next).await;
    }

    async fn write(&mut self, frame: &[u8]) -> Result<(), ()> {
        let Some(connection) = &mut self.connection else {
            return Err(());
        };
        match timeout(IO_LIMIT, connection.io.write_all(frame)).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(()),
        }
    }

    async fn send_activity(&mut self) -> Result<(), ()> {
        let ready = self.connection.as_ref().is_some_and(|c| c.ready);
        let Some((playback, received)) = self.playback.as_ref().filter(|_| ready) else {
            return Ok(());
        };
        let now = Instant::now();
        let mut playback = playback.clone();
        if playback.state == PlaybackState::Playing {
            playback.time += now.saturating_duration_since(*received).as_secs_f64();
        }
        let activity = discord_activity(&playback, self.now_ms());
        self.last_sent = Some(now);
        self.write(&set_activity(activity)).await
    }

    /// Handles every complete frame; `Err` means the socket must be dropped.
    async fn drain_frames(&mut self) -> Result<(), ()> {
        loop {
            let frame = match self.connection.as_mut().map(|c| c.reader.next_frame()) {
                Some(Ok(Some(frame))) => frame,
                Some(Ok(None)) => return Ok(()),
                _ => return Err(()),
            };
            match frame {
                (3, body) => self.write(&raw_frame(4, &body)).await?,
                (2, _) => return Err(()),
                (1, body) => {
                    let message: Value = serde_json::from_slice(&body).map_err(|_| ())?;
                    match message["evt"].as_str() {
                        Some("ERROR") => return Err(()),
                        Some("READY") => {
                            if let Some(c) = &mut self.connection {
                                c.ready = true;
                                c.deadline = None;
                            }
                            self.schedule().await;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// Clears the activity if one may be showing, then closes the socket quietly.
    async fn disconnect(&mut self) {
        self.timer = None;
        if self.connection.as_ref().is_some_and(|c| c.ready) {
            let _ = self.write(&set_activity(Value::Null)).await;
        }
        if let Some(mut connection) = self.connection.take() {
            let _ = timeout(Duration::from_secs(1), connection.io.shutdown()).await;
        }
    }
}

fn set_activity(activity: Value) -> Vec<u8> {
    discord_frame(
        1,
        &json!({
            "cmd": "SET_ACTIVITY",
            "args": {"pid": std::process::id(), "activity": activity},
            "nonce": nonce(),
        }),
    )
}
