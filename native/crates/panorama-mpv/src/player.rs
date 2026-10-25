use crate::{
    MpvError, MpvValue, PlayerEvent, PlayerOptions, PropertyValue, SeekMode,
    events::Properties,
    ffi::{Api, Client, Wake},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// Runtime loader namespace. Load on an application background thread.
pub struct Mpv;

impl Mpv {
    /// Loads only the canonical full library path. `None` selects the executable's
    /// directory (on macOS, the bundle's `Frameworks` first); this does not search
    /// PATH or read `PANORAMA_LIBMPV_DIR` implicitly.
    /// Filesystem access and the OS loader are synchronous startup operations.
    pub fn load_library(dir: Option<&Path>) -> Result<MpvLibrary, MpvError> {
        Api::load(dir).map(|api| MpvLibrary { api })
    }
}

/// Resolved runtime functions; retained until every player has terminated.
#[derive(Clone)]
pub struct MpvLibrary {
    api: Arc<Api>,
}

enum Request {
    Command(Vec<String>),
    /// `sub-add`: its failure is the subtitle's, reported as `SubtitleFailed`, not playback's.
    Subtitle(Vec<String>),
    Property(String, PropertyValue),
    Properties(Vec<(String, PropertyValue)>),
}

/// Non-blocking command sender. The worker owns all playback operations.
/// Startup failures arrive as `Error`, followed by `Shutdown`.
pub struct Player {
    commands: Sender<Request>,
    events: Option<Receiver<PlayerEvent>>,
    updates: Sender<PlayerEvent>,
    stopping: Arc<AtomicBool>,
    wake: Arc<Wake>,
    done: Option<Receiver<Result<(), MpvError>>>,
    terminal: Arc<AtomicBool>,
}

impl MpvLibrary {
    /// Starts initialization on the event thread and returns immediately.
    /// `Ready` confirms initialization. Commands may be queued before readiness.
    pub fn create(&self, options: PlayerOptions) -> Result<Player, MpvError> {
        for (name, value) in &options.extra {
            if !EXTRA_OPTIONS.contains(&name.as_str()) {
                return Err(MpvError::InvalidArgument);
            }
            validate(value)?;
        }
        let (commands, incoming) = mpsc::channel();
        let (updates, events) = mpsc::channel();
        let (completed, done) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(Wake::default());
        let terminal = Arc::new(AtomicBool::new(false));
        let api = self.api.clone();
        let worker_stop = stopping.clone();
        let worker_wake = wake.clone();
        let worker_updates = updates.clone();
        let worker = thread::Builder::new()
            .name("panorama-mpv-events".into())
            .spawn(move || {
                run(
                    api,
                    options,
                    incoming,
                    worker_updates,
                    worker_stop,
                    worker_wake,
                )
            })
            .map_err(|_| MpvError::Thread)?;
        let cleanup_wake = wake.clone();
        let cleanup_updates = updates.clone();
        let cleanup_terminal = terminal.clone();
        if thread::Builder::new()
            .name("panorama-mpv-teardown".into())
            .spawn(move || {
                let result = worker.join();
                cleanup_wake.disable();
                let status = match result {
                    Ok(client) => {
                        drop(client);
                        Ok(())
                    }
                    Err(_) => Err(MpvError::Thread),
                };
                let _ = completed.send(status);
                shutdown_event(&cleanup_updates, &cleanup_terminal);
            })
            .is_err()
        {
            stopping.store(true, Ordering::Release);
            wake.signal();
            return Err(MpvError::Thread);
        }
        Ok(Player {
            commands,
            events: Some(events),
            updates,
            stopping,
            wake,
            done: Some(done),
            terminal,
        })
    }
}

impl Player {
    /// Takes the single event receiver. Repeated calls return `EventsTaken`.
    pub fn events(&mut self) -> Result<Receiver<PlayerEvent>, MpvError> {
        self.events.take().ok_or(MpvError::EventsTaken)
    }

    /// Queues replacement playback. Input is never included in logs or errors.
    pub fn load(&self, url: &str) -> Result<(), MpvError> {
        self.load_at(url, None)
    }
    /// Queues replacement playback opening at `start` seconds, so a resume reads from its
    /// position instead of buffering the opening and seeking. The per-file option is built
    /// here from a number, never from caller text.
    pub fn load_at(&self, url: &str, start: Option<f64>) -> Result<(), MpvError> {
        match start.filter(|start| start.is_finite() && *start > 0.0) {
            Some(start) => {
                self.command(&["loadfile", url, "replace", "-1", &format!("start={start}")])
            }
            None => self.command(&["loadfile", url, "replace"]),
        }
    }
    /// Queues stop without closing the player.
    pub fn stop(&self) -> Result<(), MpvError> {
        self.command(&["stop"])
    }
    /// Queues user pause state.
    pub fn set_pause(&self, paused: bool) -> Result<(), MpvError> {
        self.property("pause", paused)
    }
    /// Queues a finite, exact seek. Absolute positions must be non-negative.
    pub fn seek(&self, seconds: f64, mode: SeekMode) -> Result<(), MpvError> {
        if !seconds.is_finite() || (matches!(mode, SeekMode::Absolute) && seconds < 0.0) {
            return Err(MpvError::InvalidArgument);
        }
        self.command(&[
            "seek",
            &seconds.to_string(),
            match mode {
                SeekMode::Absolute => "absolute+exact",
                SeekMode::Relative => "relative+exact",
            },
        ])
    }
    /// Queues a scalar property named in `PROPERTIES`; others are rejected as
    /// `InvalidArgument`. Runtime failures arrive as sanitized `Error` events.
    pub fn set_property<T: MpvValue>(&self, name: &str, value: T) -> Result<(), MpvError> {
        if !PROPERTIES.contains(&name) {
            return Err(MpvError::InvalidArgument);
        }
        self.property(name, value)
    }
    /// Queues pause toggling.
    pub fn toggle_pause(&self) -> Result<(), MpvError> {
        self.command(&["cycle", "pause"])
    }
    /// Queues an internal argv command, copied before return. Callers outside this
    /// crate use typed methods: mpv's command grammar (prefixes, quoting, per-file
    /// option lists) could otherwise reach `wid`.
    fn command(&self, args: &[&str]) -> Result<(), MpvError> {
        if args.is_empty() {
            return Err(MpvError::InvalidArgument);
        }
        for arg in args {
            validate(arg)?;
        }
        self.send(Request::Command(
            args.iter().map(|s| (*s).to_owned()).collect(),
        ))
    }
    /// Selects audio by ID, or disables audio with `None`.
    pub fn set_audio_track(&self, id: Option<i64>) -> Result<(), MpvError> {
        self.track("aid", id)
    }
    /// Selects subtitles by ID, or disables them with `None`.
    pub fn set_subtitle_track(&self, id: Option<i64>) -> Result<(), MpvError> {
        self.track("sid", id)
    }
    /// Adds a remote subtitle file to the loaded file without selecting it. Only `http(s)`
    /// URLs are accepted, so an addon cannot point mpv at local files or other protocols.
    /// Call after the file has loaded: mpv refuses tracks while nothing plays.
    pub fn add_subtitle(&self, url: &str, title: &str, lang: &str) -> Result<(), MpvError> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(MpvError::InvalidArgument);
        }
        let args = ["sub-add", url, "auto", title, lang];
        for arg in args {
            validate(arg)?;
        }
        self.send(Request::Subtitle(
            args.iter().map(|s| (*s).to_owned()).collect(),
        ))
    }
    /// Queues volume in the inclusive range 0 through 100.
    pub fn set_volume(&self, volume: f64) -> Result<(), MpvError> {
        if !(0.0..=100.0).contains(&volume) {
            return Err(MpvError::InvalidArgument);
        }
        self.property("volume", volume)
    }
    /// Applies the subtitle fallback style in one ordered batch.
    /// Text cue rendering in PR 3.3 may override mpv visibility explicitly.
    pub fn set_subtitle_style(&self, style: crate::SubtitleStyle) -> Result<(), MpvError> {
        self.send(Request::Properties(style.properties()?))
    }
    /// Signals stop and wakeup, then returns a completion receiver immediately.
    /// Poll it off GPUI; the two-second deadline includes event-thread join and
    /// termination. On timeout, the detached teardown worker retains the runtime
    /// and HWND until mpv returns. Keep the parent alive and pumping messages.
    pub fn close(mut self) -> Receiver<Result<(), MpvError>> {
        self.begin_close()
    }

    fn track(&self, name: &str, id: Option<i64>) -> Result<(), MpvError> {
        if id.is_some_and(|id| id <= 0) {
            return Err(MpvError::InvalidArgument);
        }
        self.property(
            name,
            id.map(|id| id.to_string()).unwrap_or_else(|| "no".into()),
        )
    }

    fn property<T: MpvValue>(&self, name: &str, value: T) -> Result<(), MpvError> {
        self.send(Request::Property(name.to_owned(), value.into_mpv_value()?))
    }

    fn send(&self, request: Request) -> Result<(), MpvError> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(MpvError::Closed);
        }
        self.commands.send(request).map_err(|_| MpvError::Closed)?;
        self.wake.signal();
        Ok(())
    }

    fn begin_close(&mut self) -> Receiver<Result<(), MpvError>> {
        self.begin_close_with_timeout(CLOSE_TIMEOUT)
    }

    fn begin_close_with_timeout(&mut self, timeout: Duration) -> Receiver<Result<(), MpvError>> {
        self.stopping.store(true, Ordering::Release);
        self.wake.signal();
        let (completed, result) = mpsc::channel();
        if let Some(done) = self.done.take() {
            let updates = self.updates.clone();
            let terminal = self.terminal.clone();
            let _ = thread::Builder::new()
                .name("panorama-mpv-close-deadline".into())
                .spawn(move || {
                    let status = match done.recv_timeout(timeout) {
                        Ok(status) => status,
                        Err(mpsc::RecvTimeoutError::Timeout) => Err(MpvError::ShutdownTimeout),
                        Err(mpsc::RecvTimeoutError::Disconnected) => Err(MpvError::Thread),
                    };
                    if status == Err(MpvError::ShutdownTimeout) {
                        let _ = updates.send(PlayerEvent::Error(MpvError::ShutdownTimeout));
                    } else {
                        shutdown_event(&updates, &terminal);
                    }
                    let _ = completed.send(status);
                });
        }
        result
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if self.done.is_some() {
            drop(self.begin_close());
        }
    }
}

fn run(
    api: Arc<Api>,
    options: PlayerOptions,
    incoming: Receiver<Request>,
    updates: Sender<PlayerEvent>,
    stopping: Arc<AtomicBool>,
    wake: Arc<Wake>,
) -> Option<Arc<Client>> {
    let client = match api.create(options.wid.clone()) {
        Ok(client) => client,
        Err(error) => {
            let _ = updates.send(PlayerEvent::Error(error));
            return None;
        }
    };
    wake.register(&client);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if let Err(error) = client.initialize(&options) {
            let _ = updates.send(PlayerEvent::Error(error));
            return;
        }
        let _ = updates.send(PlayerEvent::Ready);
        let mut properties = Properties::default();
        let mut pending = false;
        // The command awaiting its reply is a subtitle file's.
        let mut subtitle = false;
        let mut batch = std::collections::VecDeque::new();
        while !stopping.load(Ordering::Acquire) {
            if !pending {
                let next = if let Some((name, value)) = batch.pop_front() {
                    Some(Request::Property(name, value))
                } else {
                    incoming.try_recv().ok()
                };
                subtitle = matches!(next, Some(Request::Subtitle(_)));
                let result = match next {
                    Some(Request::Command(args) | Request::Subtitle(args)) => {
                        Some(client.command(&args))
                    }
                    Some(Request::Property(name, mut value)) => Some(client.set(&name, &mut value)),
                    Some(Request::Properties(values)) => {
                        batch.extend(values);
                        continue;
                    }
                    None => None,
                };
                if let Some(result) = result {
                    pending = result.is_ok();
                    if let Err(error) = result {
                        let _ = updates.send(if subtitle {
                            PlayerEvent::SubtitleFailed
                        } else {
                            PlayerEvent::Error(error)
                        });
                    }
                }
            }
            let received = client.receive(&mut properties);
            let subtitle_reply = received.reply && std::mem::take(&mut subtitle);
            if received.reply {
                pending = false;
            }
            for event in received.events {
                let _ = updates.send(match event {
                    PlayerEvent::Error(_) if subtitle_reply => PlayerEvent::SubtitleFailed,
                    event => event,
                });
            }
            if received.shutdown {
                break;
            }
        }
        let _ = client.command(&["stop".into()]);
    }));
    if outcome.is_err() {
        let _ = updates.send(PlayerEvent::Error(MpvError::Thread));
    }
    Some(client)
}

/// Options `PlayerOptions::extra` accepts. On macOS mpv dereferences `wid` as the
/// surface lease, and `include`, `profile`, `config-dir` or an IPC client can rewrite
/// it, so callers get closed lists rather than mpv's option and command grammar.
pub const EXTRA_OPTIONS: &[&str] = &[
    "ao",
    "audio-files",
    "cache-pause-wait",
    "http-proxy",
    "keep-open",
    "sub-font",
    "sub-fonts-dir",
    "vo",
];

/// Properties `Player::set_property` accepts; see `EXTRA_OPTIONS`.
pub const PROPERTIES: &[&str] = &[
    "audio-channels",
    "audio-device",
    "audio-spdif",
    "mute",
    "sub-visibility",
    "target-colorspace-hint",
    "target-prim",
    "target-trc",
];

fn validate(value: &str) -> Result<(), MpvError> {
    if value.contains('\0') {
        Err(MpvError::InvalidArgument)
    } else {
        Ok(())
    }
}

fn shutdown_event(updates: &Sender<PlayerEvent>, terminal: &AtomicBool) {
    if !terminal.swap(true, Ordering::AcqRel) {
        let _ = updates.send(PlayerEvent::Shutdown);
    }
}

#[cfg(test)]
mod tests;
