mod chrome;
mod content;
mod controls;
mod cue_view;
mod edit;
mod input;
pub mod keyboard;
mod measure;
mod overlays;
pub mod panel;
mod panel_motion;
mod panel_view;
mod panel_widgets;
mod picker;
pub mod pointer;
mod prefs_actions;
mod prefs_save;
mod prefs_ui;
mod range;
mod range_view;
mod settings_view;
mod stepper_view;
pub(crate) mod subtitles;
mod subtitles_view;
mod time;
mod view;
mod widgets;

use crate::{
    app::AppShell,
    app_state::AppState,
    args::Args,
    playback::{
        health::Health,
        host::{Command, PlaybackHost},
        model::{HostEvent, PlaybackError, PlaybackModel},
        selection::{PlaybackSelection, SourceRef},
        surface::{SurfaceRect, physical_rect},
    },
    transition::Tween,
};
use chrome::Chrome;
use content::Content;
use futures::StreamExt;
use gpui::{Context, Entity, ScrollHandle, Subscription, Task, WeakEntity, Window};
use measure::Measures;
use panel::Panels;
use panel_motion::PanelMotion;
use panorama_core::addons::StreamGroup;
use panorama_mpv::PlayerEvent;
use range::Scrub;
use std::{rc::Rc, sync::Arc, time::Instant};
use widgets::{Control, ControlMotion, Focuses};

struct SourcesCache {
    groups: Arc<Vec<StreamGroup>>,
    current: SourceRef,
    content: Rc<Content>,
}

impl SourcesCache {
    /// Still valid for these inputs. The cache holds its `Arc`, so a pointer match cannot be a
    /// freed allocation reused by another list.
    fn fresh(&self, groups: &Arc<Vec<StreamGroup>>, current: SourceRef) -> bool {
        Arc::ptr_eq(&self.groups, groups) && self.current == current
    }
}

pub struct Player {
    shell: WeakEntity<AppShell>,
    state: Entity<AppState>,
    args: Args,
    entry: u64,
    id: String,
    model: PlaybackModel,
    mounted: bool,
    surface_shown: bool,
    awake: crate::awake::Awake,
    focus: Focuses,
    motion: [ControlMotion; Control::COUNT],
    emphasis: [Tween; 2],
    hovered: [bool; 2],
    scrub: Scrub,
    volume_drag: bool,
    /// The last committed seek, until the host echoes it.
    seek_target: Option<f64>,
    /// A source switch asked for and not yet applied.
    pending_source: Option<SourceRef>,
    chrome: Chrome,
    shown: chrome::Shown,
    chrome_fade: Tween,
    chrome_wake: Option<Task<()>>,
    keyboard_active: bool,
    /// The centre play/pause mark: whether it shows play, and when the toggle happened.
    flash: Option<(bool, Instant)>,
    pointer: pointer::Hider,
    reduced: bool,
    panels: Panels,
    panel_motion: PanelMotion,
    /// One per panel, then the Subtitles page under its tabs.
    scrolls: [ScrollHandle; 5],
    /// The Subtitles tab indicator, in tabs from the first.
    tab_motion: Tween,
    ui: prefs_ui::PrefsUi,
    groups: Arc<Vec<StreamGroup>>,
    sources: Option<SourcesCache>,
    health: Health,
    dismissed: Option<SourceRef>,
    measures: Rc<Measures>,
    debug_ui_applied: bool,
    ui_settled: bool,
    launched: Instant,
    rect: Option<SurfaceRect>,
    #[cfg(any(windows, target_os = "macos"))]
    surface: Option<panorama_mpv::VideoSurface>,
    events: Option<Task<()>>,
    /// Addon subtitle files still being looked up when the play started.
    late_subtitles: Option<Task<()>>,
    /// Addon files not yet sent: the rest wait while the auto-pick loads, so its selection
    /// reaches mpv ahead of their downloads.
    subtitle_queue: Vec<panorama_core::addons::SubtitleSource>,
    /// External tracks mpv had when the auto-pick file was sent; set while it loads.
    subtitle_wait: Option<usize>,
    bounds: Option<Subscription>,
    blur: Option<Subscription>,
    edit_blur: Option<Subscription>,
    _shell: Subscription,
    _state: Subscription,
    _release: Subscription,
    _images: Subscription,
}

fn current_selection(state: &AppState, id: &str, args: &Args) -> Option<PlaybackSelection> {
    state
        .playback
        .as_ref()
        .filter(|selection| selection.film.id == id)
        .cloned()
        .or_else(|| crate::playback::debug::selection(id, args))
}

impl Player {
    pub fn new(
        id: String,
        entry: u64,
        shell: WeakEntity<AppShell>,
        state: Entity<AppState>,
        args: Args,
        cx: &mut Context<Self>,
    ) -> Self {
        let observer = shell
            .upgrade()
            .map(|shell| {
                cx.observe(&shell, |player, shell, cx| {
                    if !shell.read(cx).is_current(player.entry) {
                        player.unmount(cx);
                    }
                    cx.notify();
                })
            })
            .unwrap_or_else(|| cx.on_release(|_, _| {}));
        let release = cx.on_release(|player, cx| player.unmount(cx));
        let images = cx.observe(&state.read(cx).images.clone(), |_, _, cx| cx.notify());
        let streams = cx.observe(&state, |player, _, cx| {
            player.sync_groups(cx);
            cx.notify();
        });
        let selection = current_selection(state.read(cx), &id, &args);
        let mut model = PlaybackModel::new(
            selection
                .as_ref()
                .map(|selection| selection.film.name.clone())
                .or_else(|| {
                    let launch = state.read(cx).launch.as_ref();
                    launch.filter(|l| l.id == id).map(|l| l.film.name.clone())
                })
                .unwrap_or_default(),
            selection
                .as_ref()
                .map(|selection| selection.source)
                .unwrap_or_default(),
        );
        model.apply(HostEvent::Resolving);
        let now = Instant::now();
        Self {
            shell,
            state,
            args,
            entry,
            id,
            model,
            mounted: false,
            surface_shown: false,
            awake: Default::default(),
            focus: Focuses::new(cx),
            motion: [ControlMotion::new(); Control::COUNT],
            emphasis: [Tween::fixed(0.0); 2],
            hovered: [false; 2],
            scrub: Scrub::default(),
            volume_drag: false,
            seek_target: None,
            pending_source: None,
            chrome: Chrome::new(now),
            shown: chrome::Shown {
                top: true,
                bottom: true,
                pointer: true,
            },
            chrome_fade: Tween::fixed(1.0),
            chrome_wake: None,
            keyboard_active: false,
            flash: None,
            pointer: pointer::Hider::default(),
            reduced: true,
            panels: Panels::default(),
            panel_motion: PanelMotion::default(),
            scrolls: std::array::from_fn(|_| ScrollHandle::new()),
            tab_motion: Tween::fixed(0.0),
            ui: prefs_ui::PrefsUi::new(),
            groups: Arc::new(
                selection
                    .map(|selection| selection.groups)
                    .unwrap_or_default(),
            ),
            sources: None,
            health: Health::default(),
            dismissed: None,
            measures: Rc::default(),
            debug_ui_applied: false,
            ui_settled: false,
            launched: now,
            rect: None,
            #[cfg(any(windows, target_os = "macos"))]
            surface: None,
            events: None,
            late_subtitles: None,
            subtitle_queue: vec![],
            subtitle_wait: None,
            bounds: None,
            blur: None,
            edit_blur: None,
            _shell: observer,
            _state: streams,
            _release: release,
            _images: images,
        }
    }

    /// Pause or resume, flashing the new state in the middle of the picture.
    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.flash = Some((self.model.paused, Instant::now()));
        self.send(Command::TogglePause, cx);
        cx.notify();
    }

    fn send(&self, command: Command, cx: &gpui::App) {
        if let Some(host) = &self.state.read(cx).playback_host {
            host.send(command);
        }
    }

    fn selection(&self, cx: &Context<Self>) -> Option<PlaybackSelection> {
        current_selection(self.state.read(cx), &self.id, &self.args)
    }

    /// Follow Film's stream delivery while the player is open.
    fn sync_groups(&mut self, cx: &mut Context<Self>) {
        let Some((id, groups)) = self.state.read(cx).streams.clone() else {
            return;
        };
        let playing = self.selection(cx).map(|selection| selection.video_id);
        if Some(&id) != playing.as_ref() && id != self.id
            || Arc::ptr_eq(&groups, &self.groups)
            || *groups == *self.groups
        {
            return;
        }
        self.send(Command::Groups(groups.as_ref().clone()), cx);
        self.groups = groups;
        cx.notify();
    }

    fn handle_event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        let source_changed = matches!(
            event,
            HostEvent::Resolving | HostEvent::SourceChanged(_) | HostEvent::Closed
        );
        let link = match &event {
            HostEvent::Link(link) => Some(*link),
            _ => None,
        };
        if self.volume_drag && matches!(event, HostEvent::Volume(_)) {
            // An echo of an earlier step would snap the thumb back mid-drag.
            return;
        }
        match &event {
            HostEvent::Seeked(_) | HostEvent::Resolving | HostEvent::Closed => {
                self.seek_target = None
            }
            _ => {}
        }
        match &event {
            HostEvent::SourceChanged(_) | HostEvent::Failed(_) | HostEvent::Closed => {
                self.pending_source = None;
            }
            _ => {}
        }
        let tracks = matches!(&event, HostEvent::Mpv(PlayerEvent::TrackList(_)));
        // A subtitle file mpv could not load adds no track: the queue moves on without it.
        let failed = matches!(&event, HostEvent::Mpv(PlayerEvent::SubtitleFailed));
        let devices = matches!(&event, HostEvent::Mpv(PlayerEvent::AudioDeviceList(_)));
        self.model.apply(event);
        if source_changed {
            self.health = Health::default();
            self.ui.subs.reset();
        }
        if tracks || source_changed {
            self.sync_subtitles(cx);
        }
        if tracks {
            self.restore_language(cx);
        }
        if failed {
            self.subtitle_wait = None;
        }
        if tracks || failed {
            self.send_subtitles(cx);
        }
        if devices {
            self.reconcile_devices(cx);
        }
        if let Some(link) = link {
            self.health.observe(
                link,
                self.model.status == crate::playback::model::Status::Buffering,
                self.model.cache_seconds,
                self.model.rebuffers,
            );
        }
        self.sync_shell(cx);
        cx.notify();
    }

    /// Retry from the error screen. A play that never found a source has no playback to
    /// retry, so the addons are asked again and the loader shows while they answer.
    pub(super) fn retry(&mut self, cx: &mut Context<Self>) {
        let failed = self
            .state
            .read(cx)
            .launch
            .as_ref()
            .filter(|launch| launch.id == self.id && launch.failed)
            .map(|launch| {
                (
                    launch.film.clone(),
                    launch.video_id.clone(),
                    launch.resume.clone(),
                )
            });
        let Some((film, video_id, resume)) = failed else {
            self.send(Command::Retry, cx);
            return;
        };
        self.state
            .update(cx, |state, cx| state.launch(film, video_id, resume, cx));
        self.mounted = false;
        self.model.apply(HostEvent::Resolving);
        cx.notify();
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mounted {
            return;
        }
        let Some(mut selection) = self.selection(cx) else {
            let launch = self.state.read(cx).launch.as_ref();
            match launch
                .filter(|launch| launch.id == self.id)
                .map(|l| l.failed)
            {
                // Still looking up sources: the loader holds until a source is picked.
                Some(false) => return,
                Some(true) => self.model.apply(HostEvent::Failed(PlaybackError::NoSource)),
                None => self
                    .model
                    .apply(HostEvent::Failed(PlaybackError::SourceUnavailable)),
            }
            self.mounted = true;
            return;
        };
        self.mounted = true;
        self.model.title.clone_from(&selection.film.name);
        // Files already found go through the same queue as late ones, after Start.
        let early = std::mem::take(&mut selection.subtitles);
        self.load_prefs(cx);
        let fixture = if self.args.play_file.is_none()
            && self.args.play_url.is_none()
            && !self.args.play_torrent
            && self.args.fixtures
        {
            Some(
                self.args
                    .player_state
                    .unwrap_or(crate::playback::fake::FixtureState::Ready),
            )
        } else {
            None
        };
        let file = self.args.play_file.clone();
        let receiver = self.state.update(cx, |state, _| {
            if state.playback_host.is_none()
                && let Some(services) = state.services.clone()
            {
                state.playback_host = Some(PlaybackHost::new(services, fixture, file));
            }
            state.playback_host.as_mut().and_then(PlaybackHost::events)
        });
        if let Some(mut receiver) = receiver {
            self.events = Some(cx.spawn(async move |entity, cx| {
                while let Some(event) = receiver.next().await {
                    if entity
                        .update(cx, |player, cx| player.handle_event(event, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        if let Some(lookup) = selection.late_subtitles.clone() {
            self.late_subtitles = Some(cx.spawn(async move |entity, cx| {
                let files = lookup.await;
                let _ = entity.update(cx, |player, cx| player.queue_subtitles(files, cx));
            }));
        }
        #[cfg(any(windows, target_os = "macos"))]
        match panorama_mpv::VideoSurface::from_window(window) {
            Ok(surface) => {
                let raw = surface.raw_window();
                self.surface = Some(surface);
                self.sync_surface(window, cx);
                self.send(
                    Command::Start {
                        window: raw,
                        selection: Box::new(selection),
                    },
                    cx,
                );
            }
            Err(_) => self.model.apply(HostEvent::Failed(PlaybackError::Unknown)),
        }
        self.queue_subtitles(early, cx);
        #[cfg(not(any(windows, target_os = "macos")))]
        self.model
            .apply(HostEvent::Failed(PlaybackError::LibraryMissing));
        self.bounds = Some(cx.observe_window_bounds(window, |player, window, cx| {
            player.sync_surface(window, cx);
            cx.notify();
        }));
        self.blur = Some(
            cx.on_blur(&self.focus.seek.clone(), window, |player, _, cx| {
                player.commit_scrub(cx);
            }),
        );
        self.edit_blur = Some(cx.on_blur(
            &self.focus.edit.clone(),
            window,
            |player, window, cx| player.commit_edit(window, cx),
        ));
    }

    fn sync_surface(&mut self, window: &Window, cx: &mut gpui::App) {
        self.model.fullscreen = window.is_fullscreen();
        self.sync_shell(cx);
        let rect = physical_rect(
            f32::from(window.viewport_size().width),
            f32::from(window.viewport_size().height),
            window.scale_factor(),
            self.model.fullscreen,
        );
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(surface) = &self.surface {
            // On macOS this also refreshes the display's refresh rate and HDR headroom,
            // which change when the window moves to another screen at the same size.
            if self.rect != Some(rect) || cfg!(target_os = "macos") {
                let _ = surface.set_bounds(panorama_mpv::PhysicalRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                });
            }
            let visible = self.model.surface_visible();
            if visible != self.surface_shown {
                if visible {
                    surface.show();
                } else {
                    surface.hide();
                }
                self.surface_shown = visible;
            }
        }
        self.rect = Some(rect);
        self.awake.set(
            matches!(
                self.model.status,
                crate::playback::model::Status::Ready | crate::playback::model::Status::Buffering
            ) && !self.model.paused,
        );
    }

    fn sync_shell(&self, cx: &mut gpui::App) {
        self.state.update(cx, |state, cx| {
            if crate::playback::shell::ShellPlayerState::update(
                &mut state.player,
                &self.model,
                self.ui_settled,
            ) {
                cx.notify();
            }
        });
    }

    fn unmount(&mut self, cx: &mut gpui::App) {
        // Leaving while the sources are still being looked up stops the lookup.
        let id = self.id.clone();
        self.state.update(cx, |state, _| {
            if state.launch.as_ref().is_some_and(|launch| launch.id == id) {
                state.launch = None;
            }
        });
        if !self.mounted {
            return;
        }
        self.flush_prefs(cx);
        self.ui.edit = None;
        self.ui.select = None;
        self.ui.color_open = false;
        self.send(Command::Stop, cx);
        self.awake.set(false);
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(surface) = self.surface.take() {
            surface.hide();
        }
        self.events = None;
        self.chrome_wake = None;
        self.bounds = None;
        self.blur = None;
        self.edit_blur = None;
        self.rect = None;
        self.mounted = false;
        self.surface_shown = false;
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.unmount(cx);
        if window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        let id = self.id.clone();
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.back_to_film(id, window, cx));
    }

    pub fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.toggle_fullscreen();
        self.chrome.activity(Instant::now());
        self.sync_surface(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sources_cache_is_invalidated_by_a_new_list_or_a_new_current_source() {
        let groups = Arc::new(Vec::new());
        let source = SourceRef::default();
        let cache = SourcesCache {
            groups: groups.clone(),
            current: source,
            content: Rc::new(content::sources(&[], source)),
        };
        assert!(cache.fresh(&groups, source));
        assert!(cache.fresh(&groups.clone(), source));
        // An equal but distinct list is a different allocation.
        assert!(!cache.fresh(&Arc::new(Vec::new()), source));
        let other = SourceRef {
            group: 1,
            stream: 0,
        };
        assert!(!cache.fresh(&groups, other));
    }
}
