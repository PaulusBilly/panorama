use super::*;
use crate::playback::{
    output::{Channels, Video},
    subtitle_prefs::{Field, FontWeight},
};
use std::sync::Mutex;

struct FakeDiscord {
    state: Mutex<DiscordSettings>,
    fail: bool,
    calls: Mutex<Vec<(bool, std::thread::ThreadId)>>,
}

impl FakeDiscord {
    fn new(enabled: bool, available: bool, fail: bool) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(DiscordSettings { enabled, available }),
            fail,
            calls: Mutex::new(vec![]),
        })
    }
}

impl DiscordPort for FakeDiscord {
    fn settings(&self) -> Option<DiscordSettings> {
        self.state.lock().ok().map(|state| *state)
    }

    fn set_enabled(&self, enabled: bool) -> io::Result<DiscordSettings> {
        self.calls
            .lock()
            .map_err(|_| io::Error::other("poisoned"))?
            .push((enabled, std::thread::current().id()));
        if self.fail {
            return Err(io::Error::other("disk full"));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("poisoned"))?;
        state.enabled = enabled;
        Ok(*state)
    }
}

#[test]
fn preferences_load_from_and_save_to_the_store() {
    let store = Store::open_in_memory().expect("store");
    assert_eq!(load(Some(&store)), Loaded::default());
    assert_eq!(load(None), Loaded::default());
    let mut subtitles = SubtitlePreferences::default();
    subtitles.set(Field::FontSize, 36.0);
    subtitles.font_weight = FontWeight::Bold;
    let output = PlaybackSettings {
        channels: Channels::Stereo,
        video: Video::Sdr,
        ..Default::default()
    };
    assert!(save(
        &store,
        super::super::subtitle_prefs::KEY,
        &subtitles.to_bytes()
    ));
    assert!(save(&store, super::super::output::KEY, &output.to_bytes()));
    assert_eq!(load(Some(&store)), Loaded { subtitles, output });
    let key = Key::pref("subtitlePreferences").expect("key");
    assert!(store.get(&key).expect("get").is_some());
    store.set(&key, b"not json").expect("set");
    assert_eq!(load(Some(&store)).subtitles, SubtitlePreferences::default());
}

#[tokio::test]
async fn the_discord_toggle_runs_on_a_worker_and_reports_the_new_state() {
    let fake = FakeDiscord::new(false, true, false);
    let runtime = tokio::runtime::Handle::current();
    let settings = read_discord(&runtime, fake.clone())
        .await
        .expect("join")
        .expect("settings");
    let view = DiscordView::from_settings(settings);
    assert!(!view.enabled && view.available);
    let result = toggle_discord(&runtime, fake.clone(), true)
        .await
        .expect("join");
    let view = DiscordView { busy: true, ..view }.finished(result);
    assert!(view.enabled && !view.busy && view.error.is_none());
    let calls = fake.calls.lock().expect("calls").clone();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].0);
    assert_ne!(calls[0].1, std::thread::current().id());
}

#[tokio::test]
async fn a_failed_toggle_keeps_the_state_and_shows_the_retry_copy() {
    let fake = FakeDiscord::new(false, true, true);
    let runtime = tokio::runtime::Handle::current();
    let before = DiscordView::from_settings(fake.settings().expect("settings"));
    let result = toggle_discord(&runtime, fake, true).await.expect("join");
    let after = DiscordView {
        busy: true,
        ..before.clone()
    }
    .finished(result);
    assert!(!after.enabled && !after.busy);
    assert_eq!(
        after.error,
        Some("Discord setting could not be saved. Try again.")
    );
}

#[tokio::test]
async fn writes_land_in_order_so_the_newest_wins() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let runtime = tokio::runtime::Handle::current();
    let (writer, done) = PrefsWriter::spawn(&runtime, store.clone());
    for value in 0..40u8 {
        writer.save("subtitlePreferences", vec![value]);
    }
    drop(writer);
    done.await.expect("writer");
    let key = Key::pref("subtitlePreferences").expect("key");
    assert_eq!(store.get(&key).expect("get"), Some(vec![39]));
}

#[test]
fn a_toggle_is_refused_while_busy_or_unavailable() {
    let mut view = DiscordView::from_settings(DiscordSettings {
        enabled: false,
        available: true,
    });
    assert!(view.begin_toggle());
    assert!(view.busy);
    assert!(
        !view.begin_toggle(),
        "a second press while busy does nothing"
    );
    let mut unavailable = DiscordView::from_settings(DiscordSettings {
        enabled: false,
        available: false,
    });
    assert!(!unavailable.begin_toggle());
    assert!(!unavailable.busy);
    let mut failed = DiscordView {
        error: Some("old"),
        ..DiscordView::from_settings(DiscordSettings {
            enabled: true,
            available: true,
        })
    };
    assert!(failed.begin_toggle());
    assert_eq!(failed.error, None);
}

#[test]
fn the_helper_copy_follows_availability() {
    let available = DiscordView::from_settings(DiscordSettings {
        enabled: false,
        available: true,
    });
    let unavailable = DiscordView::from_settings(DiscordSettings {
        enabled: false,
        available: false,
    });
    assert_eq!(
        available.helper(),
        "Shows the film title, artwork, and playback timing while Discord is open."
    );
    assert_eq!(
        unavailable.helper(),
        "Discord sharing is unavailable in this build."
    );
}
