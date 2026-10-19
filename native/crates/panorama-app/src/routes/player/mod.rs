pub mod keyboard;
mod overlays;
mod view;

use crate::{
    app::AppShell,
    app_state::AppState,
    args::Args,
    playback::{
        host::{Command, PlaybackHost},
        model::{HostEvent, PlaybackError, PlaybackModel},
        selection::PlaybackSelection,
        surface::{SurfaceRect, physical_rect},
    },
    transition::Tween,
};
use futures::StreamExt;
use gpui::{Context, Entity, FocusHandle, Subscription, Task, WeakEntity, Window};
use std::time::Instant;

pub struct Player {
    shell: WeakEntity<AppShell>,
    state: Entity<AppState>,
    args: Args,
    entry: u64,
    id: String,
    model: PlaybackModel,
    mounted: bool,
    surface_shown: bool,
    close_focus: FocusHandle,
    primary_focus: FocusHandle,
    secondary_focus: FocusHandle,
    press: Tween,
    hover: Tween,
    launched: Instant,
    rect: Option<SurfaceRect>,
    #[cfg(windows)]
    surface: Option<panorama_mpv::win32::VideoSurface>,
    events: Option<Task<()>>,
    bounds: Option<Subscription>,
    _shell: Subscription,
    _release: Subscription,
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
        let selection = state
            .read(cx)
            .playback
            .as_ref()
            .filter(|selection| selection.film.id == id)
            .cloned();
        let mut model = PlaybackModel::new(
            selection
                .as_ref()
                .map(|selection| selection.film.name.clone())
                .unwrap_or_default(),
            selection
                .as_ref()
                .map(|selection| selection.source)
                .unwrap_or_default(),
        );
        model.apply(HostEvent::Resolving);
        Self {
            shell,
            state,
            args,
            entry,
            id,
            model,
            mounted: false,
            surface_shown: false,
            close_focus: cx.focus_handle(),
            primary_focus: cx.focus_handle(),
            secondary_focus: cx.focus_handle(),
            press: Tween::fixed(1.0),
            hover: Tween::fixed(0.0),
            launched: Instant::now(),
            rect: None,
            #[cfg(windows)]
            surface: None,
            events: None,
            bounds: None,
            _shell: observer,
            _release: release,
        }
    }

    fn send(&self, command: Command, cx: &gpui::App) {
        if let Some(host) = &self.state.read(cx).playback_host {
            host.send(command);
        }
    }

    fn selection(&self, cx: &Context<Self>) -> Option<PlaybackSelection> {
        self.state
            .read(cx)
            .playback
            .as_ref()
            .filter(|selection| selection.film.id == self.id)
            .cloned()
            .or_else(|| crate::playback::debug::selection(&self.id, &self.args))
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mounted {
            return;
        }
        self.mounted = true;
        let Some(selection) = self.selection(cx) else {
            self.model
                .apply(HostEvent::Failed(PlaybackError::SourceUnavailable));
            return;
        };
        self.model.title.clone_from(&selection.film.name);
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
                        .update(cx, |player, cx| {
                            player.model.apply(event);
                            player.sync_shell(cx);
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        #[cfg(windows)]
        match panorama_mpv::win32::VideoSurface::from_window(window) {
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
        #[cfg(not(windows))]
        self.model
            .apply(HostEvent::Failed(PlaybackError::LibraryMissing));
        self.bounds = Some(cx.observe_window_bounds(window, |player, window, cx| {
            player.sync_surface(window, cx);
            cx.notify();
        }));
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
        #[cfg(windows)]
        if let Some(surface) = &self.surface {
            if self.rect != Some(rect) {
                let _ = surface.set_bounds(panorama_mpv::win32::PhysicalRect {
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
    }

    pub fn key_action(&self, event: &gpui::KeyDownEvent) -> Option<keyboard::KeyAction> {
        keyboard::action(event, Some(&self.model))
    }

    fn sync_shell(&self, cx: &mut gpui::App) {
        self.state.update(cx, |state, cx| {
            if crate::playback::shell::ShellPlayerState::update(&mut state.player, &self.model) {
                cx.notify();
            }
        });
    }

    fn unmount(&mut self, cx: &mut gpui::App) {
        if !self.mounted {
            return;
        }
        self.send(Command::Stop, cx);
        #[cfg(windows)]
        if let Some(surface) = self.surface.take() {
            surface.hide();
        }
        self.events = None;
        self.bounds = None;
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
        self.sync_surface(window, cx);
        cx.notify();
    }
}
