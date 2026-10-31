use super::{chrome, measure::probe, panel::PanelId, range::RangeId, *};
use crate::{
    playback::model::Status,
    theme::{
        Theme,
        motion::{DURATION_FAST, EASE_DEFAULT, EASE_EDITORIAL},
    },
};
use gpui::{Render, div, prelude::*};

impl Render for Player {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (active, theme, reduced, keyboard) = self
            .shell
            .upgrade()
            .map(|shell| {
                let shell = shell.read(cx);
                (
                    shell.is_current(self.entry),
                    shell.theme,
                    shell.reduced_motion,
                    shell.keyboard_navigation,
                )
            })
            .unwrap_or((false, Theme::default(), true, false));
        if !active {
            self.unmount(cx);
            return div();
        }
        self.reduced = reduced;
        self.ui_settled = self.debug_ui_settled(Instant::now());
        self.mount(window, cx);
        self.advance_up_next(window, cx);
        self.sync_surface(window, cx);
        self.apply_debug_ui(window, cx);
        self.measures.pill.set(None);
        self.measures.seek_pill.set(None);
        let now = Instant::now();
        let preparing = self.model.initial_preparing();
        let fixture_canvas = self.args.fixtures
            && self.args.play_file.is_none()
            && self.args.play_url.is_none()
            && !self.args.play_torrent;
        let status = self.model.status;
        let shown = self.chrome.shown(
            now,
            chrome::Inputs {
                ready: status == Status::Ready,
                controls_ready: matches!(status, Status::Ready | Status::Buffering),
                initial_preparing: preparing,
                keyboard_focus: chrome::keyboard_focus(
                    self.keyboard_active,
                    self.focus.chrome_focused(window),
                ),
                panel_open: self.panels.active().is_some(),
                paused: self.model.paused,
                dragging: self.scrub.dragging() || self.volume_drag,
            },
        );
        self.shown = shown;
        self.sync_pointer(shown, window, cx);
        self.chrome_fade.retarget(
            if shown.top { 1.0 } else { 0.0 },
            DURATION_FAST,
            Some(EASE_DEFAULT),
            now,
        );
        let fade = self.chrome_fade.value(now, reduced);
        crate::window_controls::set_opacity(window, fade);
        self.sync_emphasis(window, now);
        self.schedule_chrome_wake(now, cx);
        let animated = preparing || matches!(status, Status::Preparing(_) | Status::Buffering);
        if (!reduced && animated)
            || self.motion.iter().any(|motion| motion.moving(now, reduced))
            || self.emphasis.iter().any(|tween| tween.moving(now, reduced))
            || self.chrome_fade.moving(now, reduced)
            || self.panel_motion.moving(now, reduced)
            || self.tab_motion.moving(now, reduced)
            || self.flash_moving(now)
        {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        if self.args.open_control.as_deref() == Some("scroll") && self.debug_ui_applied {
            self.scrolls[3].scroll_to_bottom();
        }
        let panel = self.panel_surface(theme, keyboard, window, cx);
        let popups = self.popups(theme, cx);
        let cue = self.cue_overlay();
        div()
            .relative()
            .size_full()
            .text_color(theme.ink)
            .when(
                fixture_canvas
                    || matches!(status, Status::Idle | Status::Error(_))
                    || (status == Status::NoPeers && !self.model.has_reached_playable),
                |root| root.bg(theme.player_canvas),
            )
            .child(self.input_layer(cx))
            .child(probe(&self.measures, |measures, bounds| {
                measures.root.set(bounds)
            }))
            .child(
                div()
                    .id("player-video")
                    .absolute()
                    .inset_0()
                    .on_click(cx.listener(|player, _, _, cx| {
                        if matches!(player.model.status, Status::Ready | Status::Buffering)
                            && !player.panels.swallows_click()
                        {
                            player.toggle_pause(cx);
                        }
                    })),
            )
            .when(preparing, |root| {
                root.child(
                    div()
                        .id("player-initial-preparing")
                        .absolute()
                        .inset_0()
                        .bg(theme.player_canvas)
                        .flex()
                        .items_center()
                        .justify_center()
                        .role(gpui::Role::Status)
                        .aria_label("Preparing playback")
                        .child(
                            div()
                                .relative()
                                .child(crate::cascade_loader::render(
                                    now.duration_since(self.launched).as_secs_f32(),
                                    reduced,
                                    theme,
                                ))
                                .when(self.model.torrent.is_some(), |loader| {
                                    loader.child(self.torrent_preparing_text(theme))
                                }),
                        ),
                )
            })
            .when(!preparing, |root| {
                let notice = self.heavy_notice(theme, keyboard, window, cx);
                let root = root
                    .when_some(cue, |root, cue| root.child(cue))
                    .child(self.overlays(theme, reduced, keyboard, window, cx))
                    .children(self.up_next_card(theme, cx))
                    .when_some(self.pause_flash(theme, reduced, now), |root, flash| {
                        root.child(flash)
                    })
                    .when_some(notice, |root, notice| root.child(notice));
                let root = if shown.top || fade > 0.001 {
                    root.child(self.top_bar(theme, keyboard, fade, window, cx))
                } else {
                    root
                };
                if shown.bottom {
                    root.child(self.bottom_bar(theme, keyboard, window, cx))
                } else {
                    root
                }
            })
            .when_some(panel, |root, panel| root.child(panel))
            .when_some(popups, |root, popups| root.child(popups))
    }
}

impl Player {
    fn sync_emphasis(&mut self, window: &Window, now: Instant) {
        for id in [RangeId::Volume, RangeId::Seek] {
            let (handle, disabled, dragging, forced) = match id {
                RangeId::Volume => (&self.focus.volume, false, self.volume_drag, false),
                RangeId::Seek => (
                    &self.focus.seek,
                    self.model.duration.is_none(),
                    self.scrub.dragging(),
                    self.args.hover_seek,
                ),
            };
            let on = !disabled
                && (self.hovered[id as usize] || handle.is_focused(window) || dragging || forced);
            self.emphasis[id as usize].retarget(
                if on { 1.0 } else { 0.0 },
                DURATION_FAST,
                Some(EASE_EDITORIAL),
                now,
            );
        }
    }

    /// Wake once when the auto-hide timer runs out; moves only push the deadline.
    fn schedule_chrome_wake(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(left) = self.chrome.deadline(now) else {
            return;
        };
        if self.chrome_wake.is_some() {
            return;
        }
        self.chrome_wake = Some(cx.spawn(async move |entity, cx| {
            cx.background_executor().timer(left).await;
            let _ = entity.update(cx, |player, cx| {
                player.chrome_wake = None;
                cx.notify();
            });
        }));
    }

    /// Whether capture flags and the animations they start have finished.
    fn debug_ui_settled(&self, now: Instant) -> bool {
        let requested = self.args.open_panel.is_some() || self.args.fullscreen;
        !requested
            || (self.debug_ui_applied
                && (self.args.open_panel.is_none()
                    || (self.panels.active().is_some()
                        && !self.panel_motion.moving(now, self.reduced)))
                && (self.args.open_panel != Some(PanelId::Settings) || self.ui.discord.is_some())
                && (matches!(self.args.open_control.as_deref(), None | Some("scroll"))
                    || self.ui.edit.is_some()
                    || (self.ui.select.is_some() && self.measures.popup.get().is_some()))
                && (!self.args.fullscreen || self.model.fullscreen))
    }

    /// Apply the capture flags once the chrome has been measured.
    fn apply_debug_ui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.debug_ui_applied
            || !matches!(
                self.model.status,
                Status::Ready
                    | Status::Buffering
                    | Status::Ended
                    | Status::NoPeers
                    | Status::Error(_)
            )
            || self.measures.root.get().size.width <= gpui::px(0.0)
            || (matches!(self.model.status, Status::Ready | Status::Buffering)
                && self.measures.pill.get().is_none())
        {
            return;
        }
        self.debug_ui_applied = true;
        if let Some(panel) = self.args.open_panel {
            self.show_panel(panel, window, cx);
        }
        match self.args.open_control.as_deref() {
            Some("edit") => self.begin_edit(
                crate::playback::subtitle_prefs::Field::TextOpacity,
                window,
                cx,
            ),
            Some(control @ ("color" | "appearance")) => {
                self.ui.subs.tab = super::subtitles::Tab::Settings;
                self.tab_motion = crate::transition::Tween::fixed(2.0);
                if control == "color" {
                    self.open_color(window, cx);
                }
            }
            Some("select") => self.toggle_select(super::prefs_ui::SelectId::Device, cx),
            Some("scroll") => self.scrolls[3].scroll_to_bottom(),
            _ => {}
        }
        if self.args.fullscreen {
            self.toggle_fullscreen(window, cx);
        }
    }
}
