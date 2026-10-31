//! Preference storage on blocking workers: load once, save debounced, never on the GPUI thread.
use super::{output::PlaybackSettings, subtitle_prefs::SubtitlePreferences};
use crate::services::Services;
use panorama_core::{
    discord::DiscordSettings,
    store::{Key, Store},
};
use std::{io, sync::Arc, time::Duration};

/// Wait after the last change before the preferences are written.
pub const SAVE_DELAY: Duration = Duration::from_millis(300);

/// Both preference groups, as loaded at playback start.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    /// Subtitle appearance.
    pub subtitles: SubtitlePreferences,
    /// Audio and video output.
    pub output: PlaybackSettings,
}

/// Read both preferences. A missing or unreadable store gives the defaults.
pub fn load(store: Option<&Store>) -> Loaded {
    let read = |name: &str| {
        let key = Key::pref(name).ok()?;
        store?.get(&key).ok().flatten()
    };
    Loaded {
        subtitles: read(super::subtitle_prefs::KEY)
            .map(|bytes| SubtitlePreferences::from_bytes(&bytes))
            .unwrap_or_default(),
        output: read(super::output::KEY)
            .map(|bytes| PlaybackSettings::from_bytes(&bytes))
            .unwrap_or_default(),
    }
}

/// Write a preference value by name.
pub fn save(store: &Store, name: &str, bytes: &[u8]) -> bool {
    Key::pref(name)
        .ok()
        .is_some_and(|key| store.set(&key, bytes).is_ok())
}

/// A single ordered writer: every save goes through one task, so an older write can never land
/// after a newer one (a debounce firing, then the flush when the player closes).
#[derive(Clone)]
pub struct PrefsWriter {
    jobs: tokio::sync::mpsc::UnboundedSender<(&'static str, Vec<u8>)>,
}

impl PrefsWriter {
    /// Start the writer on `runtime`; the handle ends once every sender is dropped.
    pub fn spawn(
        runtime: &tokio::runtime::Handle,
        store: Arc<Store>,
    ) -> (Self, tokio::task::JoinHandle<()>) {
        let (jobs, mut incoming) =
            tokio::sync::mpsc::unbounded_channel::<(&'static str, Vec<u8>)>();
        let handle = runtime.spawn(async move {
            while let Some((name, bytes)) = incoming.recv().await {
                let store = store.clone();
                let saved = tokio::task::spawn_blocking(move || save(&store, name, &bytes))
                    .await
                    .unwrap_or(false);
                if !saved {
                    // The key only: never the values.
                    crate::debug::log(format!("preference save failed key={name}"));
                }
            }
        });
        (Self { jobs }, handle)
    }

    /// Queue a write; it runs after every earlier one.
    pub fn save(&self, name: &'static str, bytes: Vec<u8>) {
        let _ = self.jobs.send((name, bytes));
    }
}

/// The Discord opt-in the settings panel toggles. `Services` provides it; tests use a fake.
pub trait DiscordPort: Send + Sync + 'static {
    /// The current opt-in and availability, if presence exists at all.
    fn settings(&self) -> Option<DiscordSettings>;
    /// Persist and apply the opt-in.
    fn set_enabled(&self, enabled: bool) -> io::Result<DiscordSettings>;
}

impl DiscordPort for Services {
    fn settings(&self) -> Option<DiscordSettings> {
        self.discord.as_ref().map(|discord| discord.settings())
    }

    fn set_enabled(&self, enabled: bool) -> io::Result<DiscordSettings> {
        self.discord
            .as_ref()
            .ok_or_else(|| io::Error::other("Discord presence is unavailable"))?
            .set_enabled(enabled)
    }
}

/// What the settings panel shows for Discord.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscordView {
    /// Opt-in state.
    pub enabled: bool,
    /// Whether the build can share at all.
    pub available: bool,
    /// A toggle is being applied.
    pub busy: bool,
    /// A sanitized failure line.
    pub error: Option<&'static str>,
}

impl DiscordView {
    /// The state a settings read produced.
    pub fn from_settings(settings: DiscordSettings) -> Self {
        Self {
            enabled: settings.enabled,
            available: settings.available,
            busy: false,
            error: None,
        }
    }

    /// The helper copy under the checkbox.
    pub fn helper(&self) -> &'static str {
        if self.available {
            "Shows the film title, artwork, and playback timing while Discord is open."
        } else {
            "Discord sharing is unavailable in this build."
        }
    }

    /// Start a toggle: refused (false) while one is running or when sharing is unavailable.
    pub fn begin_toggle(&mut self) -> bool {
        if self.busy || !self.available {
            return false;
        }
        self.busy = true;
        self.error = None;
        true
    }

    /// The result of a toggle on the worker.
    pub fn finished(&self, result: io::Result<DiscordSettings>) -> Self {
        match result {
            Ok(settings) => Self::from_settings(settings),
            Err(_) => Self {
                busy: false,
                error: Some("Discord setting could not be saved. Try again."),
                ..self.clone()
            },
        }
    }
}

/// Toggle on the blocking pool (the presence write is a file operation).
pub fn toggle_discord(
    runtime: &tokio::runtime::Handle,
    port: Arc<dyn DiscordPort>,
    enabled: bool,
) -> tokio::task::JoinHandle<io::Result<DiscordSettings>> {
    runtime.spawn_blocking(move || port.set_enabled(enabled))
}

/// Read the opt-in on the blocking pool.
pub fn read_discord(
    runtime: &tokio::runtime::Handle,
    port: Arc<dyn DiscordPort>,
) -> tokio::task::JoinHandle<Option<DiscordSettings>> {
    runtime.spawn_blocking(move || port.settings())
}

#[cfg(test)]
#[path = "prefs_tests.rs"]
mod tests;
