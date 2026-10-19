//! Pointer, keyboard and panel actions of the player.
use super::edit::Target;
use super::{
    Player, SourcesCache,
    content::{self, Content, RowKey},
    keyboard::{self, FocusZone, KeyAction, Route, Shortcut},
    panel::{Change, PanelId, initial_row, next_row},
    panel_motion::{FALLBACK_BOTTOM, PILL_GAP},
    pointer,
    range::{KEY_STEP, RangeId},
    widgets::Control,
};
use crate::playback::host::Command;
use gpui::{
    Canvas, Context, DispatchPhase, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Styled, Window, canvas,
};
use std::{rc::Rc, time::Instant};

impl Player {
    /// An invisible full-frame element that listens to the pointer wherever it is.
    pub(super) fn input_layer(&self, cx: &mut Context<Self>) -> Canvas<()> {
        let weak = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let player = weak.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = player.update(cx, |player, cx| {
                            player.pointer_moved(event, window, cx);
                        });
                    }
                });
                let player = weak.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let _ = player.update(cx, |player, cx| {
                            player.pointer_pressed(event.position, window, cx);
                        });
                    }
                });
                let player = weak.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let _ = player.update(cx, |player, cx| player.pointer_released(cx));
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }

    fn pointer_moved(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = event.position;
        match self.pointer.on_move(position) {
            pointer::MoveAction::Wake => {
                self.keyboard_active = false;
                self.chrome.activity(Instant::now());
            }
            pointer::MoveAction::Rehide => self.rehide_pointer(window, cx),
            pointer::MoveAction::Ignore => {}
        }
        if (self.scrub.dragging() || self.volume_drag)
            && event.pressed_button != Some(MouseButton::Left)
        {
            // The release happened outside the window: end the drag like a release.
            self.pointer_released(cx);
        } else if self.scrub.dragging() {
            self.drag_to(RangeId::Seek, position, cx);
        } else if self.volume_drag {
            self.drag_to(RangeId::Volume, position, cx);
        } else if !(self.shown.top || self.shown.bottom) {
            cx.notify();
        }
    }

    fn pointer_pressed(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keyboard_active = false;
        self.chrome.activity(Instant::now());
        let hit = |bounds: Option<gpui::Bounds<Pixels>>| {
            bounds.is_some_and(|bounds| bounds.contains(&position))
        };
        let on_popup = hit(self.measures.popup.get());
        if let Some(edit) = &self.ui.edit {
            // A press outside the field is a blur: it commits.
            let inside = match edit.target {
                Target::Field(_) => hit(self.measures.edit.get()),
                Target::Hex => on_popup || hit(self.measures.color_row.get()),
            };
            if !inside {
                self.commit_edit(window, cx);
            }
        }
        if let Some(open) = self.ui.select
            && !on_popup
        {
            self.ui.select = None;
            if hit(self.measures.selects[open.id.index()].get()) {
                self.ui.select_pressed_closed = Some(open.id);
            }
        }
        let outside = self.panels.active().is_some()
            && !on_popup
            && self
                .measures
                .surface
                .get()
                .is_none_or(|surface| !surface.contains(&position));
        if let Some(change) = self.panels.press(outside) {
            self.apply_change(change, window, cx);
        }
    }

    fn pointer_released(&mut self, cx: &mut Context<Self>) {
        // The trigger's click is dispatched during this same event; forget the press after it.
        let weak = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = weak.update(cx, |player, _| {
                player.panels.release_press();
                player.ui.select_pressed_closed = None;
            });
        });
        self.volume_drag = false;
        if self.scrub.dragging() {
            self.commit_scrub(cx);
        }
    }

    /// A press on a range: focus it and start dragging from the pointer.
    pub(super) fn range_press(
        &mut self,
        id: RangeId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match id {
            RangeId::Volume => {
                window.focus(&self.focus.volume, cx);
                self.volume_drag = true;
            }
            RangeId::Seek => {
                window.focus(&self.focus.seek, cx);
                if self.model.duration.is_none() {
                    return;
                }
                self.scrub.begin(0.0, self.model.duration);
            }
        }
        self.drag_to(id, position, cx);
    }

    fn drag_to(&mut self, id: RangeId, position: Point<Pixels>, cx: &mut Context<Self>) {
        let bounds = self.measures.ranges[id as usize].get();
        let fraction = id.look().fraction_at(
            f32::from(position.x - bounds.left()),
            f32::from(bounds.size.width),
        );
        match id {
            RangeId::Volume => self.set_volume((fraction * 100.0).round() as u8, cx),
            RangeId::Seek => {
                if let Some(duration) = self.model.duration {
                    self.scrub
                        .preview(f64::from(fraction) * duration, Some(duration));
                }
            }
        }
        cx.notify();
    }

    fn set_volume(&mut self, volume: u8, cx: &mut Context<Self>) {
        let volume = volume.min(100);
        if volume != self.model.volume {
            self.model.volume = volume;
            self.send(Command::SetVolume(volume), cx);
        }
    }

    /// Seek to the previewed time, if any.
    pub(super) fn commit_scrub(&mut self, cx: &mut Context<Self>) {
        if let Some(seconds) = self.scrub.commit() {
            self.seek_target = Some(seconds);
            self.send(Command::Seek(seconds), cx);
        }
        cx.notify();
    }

    fn step_range(&mut self, id: RangeId, key: &str, cx: &mut Context<Self>) -> bool {
        let sign = match key {
            "left" | "down" => -1.0,
            "right" | "up" => 1.0,
            "home" | "end" => 0.0,
            _ => return false,
        };
        match id {
            RangeId::Volume => {
                let volume = match key {
                    "home" => 0.0,
                    "end" => 100.0,
                    _ => f64::from(self.model.volume) + sign * KEY_STEP,
                };
                self.set_volume(volume.round().clamp(0.0, 100.0) as u8, cx);
            }
            RangeId::Seek => {
                let Some(duration) = self.model.duration else {
                    return true;
                };
                let seconds = match key {
                    "home" => 0.0,
                    "end" => duration,
                    _ => {
                        // Presses before the host echoes the last seek build on it.
                        let base = self.seek_target.unwrap_or(self.model.time.unwrap_or(0.0));
                        self.scrub.shown(base) + sign * KEY_STEP
                    }
                };
                self.scrub.preview(seconds, Some(duration));
                self.commit_scrub(cx);
            }
        }
        cx.notify();
        true
    }

    /// Offer a key to the player; `None` leaves it to the rest of the window.
    pub fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<KeyAction> {
        if pointer::is_sentinel(event) {
            // The idle-pointer hide: not activity, and nothing else may see it.
            return Some(KeyAction::Handled);
        }
        if keyboard::reserved(event) {
            return None;
        }
        let key = event.keystroke.key.as_str();
        self.keyboard_active = true;
        self.panels.release_press();
        self.ui.select_pressed_closed = None;
        self.chrome.activity(Instant::now());
        cx.notify();
        if self.ui.edit.is_some() {
            // The editor owns every key (Escape cancels it) until it closes.
            return self
                .edit_key(key, event.keystroke.key_char.as_deref(), window, cx)
                .then_some(KeyAction::Handled);
        }
        if self.ui.select.is_some() {
            if key == "escape" {
                self.ui.select = None;
                return Some(KeyAction::Handled);
            }
            return self.select_key(key, cx).then_some(KeyAction::Handled);
        }
        if key == "escape" {
            if self.scrub.dragging() || self.scrub.pending() || self.volume_drag {
                self.scrub.cancel();
                self.volume_drag = false;
                return Some(KeyAction::Handled);
            }
            let in_panel = self.focus.panel.contains_focused(window, cx);
            if let Some(restore) = self.panels.escape(in_panel) {
                self.apply_change(Change::Closed, window, cx);
                if let Some(panel) = restore {
                    window.focus(&self.focus.controls[Control::trigger(panel) as usize], cx);
                }
                return Some(KeyAction::Handled);
            }
            return Some(KeyAction::Close);
        }
        let zone = if self.focus.panel.contains_focused(window, cx) {
            FocusZone::Panel
        } else if self.focus.volume.is_focused(window) {
            FocusZone::Range(RangeId::Volume)
        } else if self.focus.seek.is_focused(window) {
            FocusZone::Range(RangeId::Seek)
        } else if self.focus.button_focused(window) {
            FocusZone::Button
        } else {
            FocusZone::Nothing
        };
        match keyboard::route(key, zone, self.panels.active().is_some()) {
            Route::Panel => return self.panel_key(key, window, cx),
            Route::Range(id) => {
                return self.step_range(id, key, cx).then_some(KeyAction::Handled);
            }
            // The focused button activates itself when the key is released.
            Route::Button => return None,
            Route::Shortcut => {}
        }
        match keyboard::shortcut(key, &self.model)? {
            Shortcut::Command(command) => self.send(command, cx),
            Shortcut::Fullscreen => self.toggle_fullscreen(window, cx),
        }
        Some(KeyAction::Handled)
    }

    fn panel_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<KeyAction> {
        let id = self.panels.active()?;
        let content = self.panel_content(id)?;
        match key {
            "up" | "left" | "down" | "right" => {
                let delta = if matches!(key, "up" | "left") { -1 } else { 1 };
                self.panels.row = next_row(self.panels.row, delta, &content.enabled());
                if let Some(row) = self.panels.row
                    && let Some(item) = content
                        .items()
                        .iter()
                        .position(|item| *item == content::Item::Row(row))
                {
                    self.scrolls[id as usize].scroll_to_item(item);
                }
                cx.notify();
                Some(KeyAction::Handled)
            }
            "enter" | "space" | " " => {
                if let Some(row) = self.panels.row.and_then(|row| content.row(row)).cloned()
                    && !row.disabled
                {
                    self.select_row(row.key, window, cx);
                }
                Some(KeyAction::Handled)
            }
            _ => None,
        }
    }

    /// The rows a panel shows now; the sources page is rebuilt only when its inputs change.
    pub(super) fn panel_content(&mut self, id: PanelId) -> Option<Rc<Content>> {
        match id {
            PanelId::Audio => Some(Rc::new(content::audio(&self.model))),
            PanelId::Sources => {
                let current = self.model.source;
                if let Some(cache) = &self.sources
                    && cache.fresh(&self.groups, current)
                {
                    return Some(cache.content.clone());
                }
                let content = Rc::new(content::sources(&self.groups, current));
                self.sources = Some(SourcesCache {
                    groups: self.groups.clone(),
                    current,
                    content: content.clone(),
                });
                Some(content)
            }
            PanelId::Subtitles => Some(Rc::new(content::subtitles(
                &self.ui.subs.view(&self.model.subtitles),
            ))),
            PanelId::Settings => None,
        }
    }

    /// Activate a row: select an audio track, or switch source and close the panel.
    pub(super) fn select_row(&mut self, key: RowKey, window: &mut Window, cx: &mut Context<Self>) {
        match key {
            RowKey::Audio(id) => {
                for track in &mut self.model.audio {
                    track.selected = track.id == id;
                }
                self.send(Command::SelectAudio(id), cx);
            }
            RowKey::Language(choice) => self.choose_language(choice, cx),
            RowKey::Subtitle(id) => self.choose_subtitle(id, cx),
            RowKey::Source(source) => {
                if source != self.model.source && self.pending_source != Some(source) {
                    self.pending_source = Some(source);
                    self.send(Command::SwitchSource(source), cx);
                }
                if let Some(change) = self.panels.close() {
                    self.apply_change(change, window, cx);
                }
            }
        }
        cx.notify();
    }

    /// A trigger activation.
    pub(super) fn toggle_panel(
        &mut self,
        id: PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(change) = self.panels.toggle(id, self.keyboard_active) {
            self.apply_change(change, window, cx);
        }
    }

    /// Open a panel from the overlays and the heavy-source notice.
    pub(super) fn show_panel(&mut self, id: PanelId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(change) = self.panels.open(id, self.keyboard_active) {
            self.apply_change(change, window, cx);
        }
    }

    pub(super) fn apply_change(
        &mut self,
        change: Change,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        if matches!(change, Change::Closed | Change::Switched { .. }) {
            self.ui.select = None;
            self.ui.edit = None;
            self.ui.color_open = false;
        }
        if self.panels.active() == Some(PanelId::Settings) && change != Change::Closed {
            self.refresh_discord(cx);
        }
        match change {
            Change::Opened => {
                let root = self.measures.root.get();
                self.panel_motion.bottom = self
                    .measures
                    .pill
                    .get()
                    .filter(|pill| pill.size.height > gpui::px(0.0))
                    .or(self.measures.seek_pill.get())
                    .map_or(FALLBACK_BOTTOM, |anchor| {
                        f32::from(root.bottom() - anchor.top()) + PILL_GAP
                    });
                self.panel_motion.apply(change, None, now);
            }
            Change::Switched { .. } => {
                if let Some(to) = self.panels.active() {
                    self.measures.pages[to as usize].set(None);
                }
                let from = self.measures.surface.get().map(|surface| surface.size);
                self.panel_motion.apply(change, from, now);
            }
            Change::Closed => self.panel_motion.apply(change, None, now),
        }
        if matches!(change, Change::Opened) && self.panels.take_focus_on_open() {
            window.focus(&self.focus.panel, cx);
            if let Some(content) = self.panels.active().and_then(|id| self.panel_content(id)) {
                self.panels.row = initial_row(content.selected(), &content.enabled());
            }
        }
        cx.notify();
    }
}
