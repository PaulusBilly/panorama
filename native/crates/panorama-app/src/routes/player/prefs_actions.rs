//! Actions of the subtitle and settings panels: loading, applying live, saving off the GPUI thread.
use super::{
    Player,
    edit::{Edit, Outcome, Target},
    prefs_ui::{OpenSelect, SelectId, apply_select, select_items},
    subtitles::Choice,
};
use crate::playback::{
    host::Command,
    output::{self, PlaybackSettings},
    prefs::{self, Loaded},
    subtitle_prefs::{self, Field, FontWeight, Rgb},
    subtitle_render::{self, Renderer, SubtitleSetup, renderer_for},
};
use gpui::{Context, Window};
use std::sync::Arc;

impl Player {
    pub(super) fn services(&self, cx: &gpui::App) -> Option<Arc<crate::services::Services>> {
        self.state.read(cx).services.clone()
    }

    /// Read the stored preferences on a blocking worker once playback starts.
    pub(super) fn load_prefs(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.services(cx) else {
            self.prefs_loaded(Loaded::default(), cx);
            return;
        };
        let runtime = services.runtime.clone();
        let store = services.store.clone();
        cx.spawn(async move |entity, cx| {
            let loaded = runtime
                .spawn_blocking(move || prefs::load(store.as_deref()))
                .await
                .unwrap_or_default();
            let _ = entity.update(cx, |player, cx| player.prefs_loaded(loaded, cx));
        })
        .detach();
    }

    fn prefs_loaded(&mut self, loaded: Loaded, cx: &mut Context<Self>) {
        self.ui.prefs = loaded.subtitles;
        if let Some(prefs) = self.args.subtitle_prefs {
            self.ui.prefs = prefs;
        }
        self.ui.output = self.args.playback_settings.clone().unwrap_or(loaded.output);
        // Capture overrides must never reach the real store.
        self.ui.ephemeral =
            self.args.subtitle_prefs.is_some() || self.args.playback_settings.is_some();
        self.ui.loaded = true;
        self.sync_subtitles(cx);
        self.restore_language(cx);
        self.reconcile_devices(cx);
        if self.ui.output != PlaybackSettings::default() {
            self.apply_output(cx);
        }
        cx.notify();
    }

    /// The renderer of the selected subtitle track.
    pub(super) fn renderer(&self) -> Renderer {
        match self.model.subtitles.iter().find(|track| track.selected) {
            None => Renderer::Gpui,
            // A selected track of unknown codec is left to mpv.
            Some(track) => renderer_for(Some(track.codec.as_deref().unwrap_or(""))),
        }
    }

    /// Tell the host when the preferences or the selected track's renderer changed.
    pub(super) fn sync_subtitles(&mut self, cx: &mut Context<Self>) {
        if !self.ui.loaded {
            return;
        }
        let Some(state) = subtitle_render::plan(self.ui.applied, &self.ui.prefs, self.renderer())
        else {
            return;
        };
        self.ui.applied = Some(state);
        self.send(
            Command::Subtitles(Box::new(SubtitleSetup::new(&state.0, state.1))),
            cx,
        );
    }

    /// A saved output that is no longer present falls back to the system default.
    pub(super) fn reconcile_devices(&mut self, cx: &mut Context<Self>) {
        if !self.ui.loaded || self.model.devices.is_empty() {
            return;
        }
        let mut settings = self.ui.output.clone();
        if let Some(notice) = output::reconcile(&mut settings, &self.model.devices) {
            self.ui.output = settings;
            self.apply_output(cx);
            self.schedule_save(1, cx);
            self.ui.notice = Some(notice);
            cx.notify();
        }
    }

    fn apply_output(&mut self, cx: &mut Context<Self>) {
        let devices = output::select_devices(&self.model.devices);
        self.send(
            Command::Output(output::properties(
                &self.ui.output,
                &devices,
                output::HINT_SUPPORTED,
            )),
            cx,
        );
    }

    fn set_output(&mut self, settings: PlaybackSettings, cx: &mut Context<Self>) {
        self.ui.output = settings;
        self.ui.notice = None;
        self.apply_output(cx);
        self.schedule_save(1, cx);
        cx.notify();
    }

    /// One minus or plus press on a stepper.
    pub(super) fn adjust(&mut self, field: Field, direction: i32, cx: &mut Context<Self>) {
        self.ui.prefs.step(field, direction);
        self.prefs_changed(field, cx);
    }

    /// Set a stepper from a typed value.
    pub(super) fn set_field(&mut self, field: Field, value: f64, cx: &mut Context<Self>) {
        self.ui.prefs.set(field, value);
        self.prefs_changed(field, cx);
    }

    /// Choose a text colour.
    pub(super) fn set_color(&mut self, color: Rgb, cx: &mut Context<Self>) {
        self.ui.prefs.text_color = color;
        self.prefs_changed(Field::TextOpacity, cx);
    }

    /// Choose a font weight.
    pub(super) fn set_weight(&mut self, weight: FontWeight, cx: &mut Context<Self>) {
        self.ui.prefs.font_weight = weight;
        self.prefs_changed(Field::FontSize, cx);
    }

    fn prefs_changed(&mut self, field: Field, cx: &mut Context<Self>) {
        self.sync_subtitles(cx);
        if field != Field::Delay {
            self.schedule_save(0, cx);
        }
        cx.notify();
    }

    /// Pick a language in the Language column.
    pub(super) fn choose_language(&mut self, choice: Choice, cx: &mut Context<Self>) {
        let selection = self.ui.subs.choose(choice, &self.model.subtitles);
        if let Some(id) = selection {
            self.select_subtitle(id, cx);
        }
        let language = match choice {
            Choice::Off => None,
            Choice::Lang(lang) => Some(lang),
        };
        if self.ui.prefs.language != language {
            self.ui.prefs.language = language;
            self.schedule_save(0, cx);
        }
        cx.notify();
    }

    /// Turn on the remembered language once its track shows up: built-in tracks arrive with
    /// the file, an addon's a few seconds later.
    pub(super) fn restore_language(&mut self, cx: &mut Context<Self>) {
        let Some(lang) = self.ui.prefs.language.filter(|_| self.ui.loaded) else {
            return;
        };
        if let Some(id) = self.ui.subs.restore(lang, &self.model.subtitles) {
            self.select_subtitle(Some(id), cx);
        }
    }

    /// Pick a track in the Track column.
    pub(super) fn choose_subtitle(&mut self, id: i64, cx: &mut Context<Self>) {
        self.ui.subs.choose_track(id, &self.model.subtitles);
        self.select_subtitle(Some(id), cx);
        cx.notify();
    }

    fn select_subtitle(&mut self, id: Option<i64>, cx: &mut Context<Self>) {
        for track in &mut self.model.subtitles {
            track.selected = Some(track.id) == id;
        }
        // The old cue belongs to the old selection; mpv sends the next one.
        self.model.cue = None;
        self.send(Command::SelectSubtitle(id), cx);
        self.sync_subtitles(cx);
    }

    /// Start editing a stepper's value.
    pub(super) fn begin_edit(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        if !field.editable() {
            return;
        }
        self.commit_edit(window, cx);
        self.ui.edit = Some(Edit::field(field, self.ui.prefs.get(field)));
        window.focus(&self.focus.edit, cx);
        cx.notify();
    }

    /// Commit the open edit (Enter, blur, or a press elsewhere).
    pub(super) fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.ui.edit.take() else {
            return;
        };
        if let (Target::Field(field), Some(value)) = (edit.target, edit.committed()) {
            self.set_field(field, value, cx);
        }
        if edit.target == Target::Hex {
            self.ui.color_open = false;
        }
        if self.focus.edit.is_focused(window) {
            window.focus(&self.focus.panel, cx);
        }
        cx.notify();
    }

    /// Escape: drop the edit without applying it.
    pub(super) fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(edit) = self.ui.edit.take() {
            if edit.target == Target::Hex {
                self.ui.color_open = false;
            }
            if self.focus.edit.is_focused(window) {
                window.focus(&self.focus.panel, cx);
            }
            cx.notify();
        }
    }

    /// Open the custom colour popover.
    pub(super) fn open_color(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_edit(window, cx);
        self.ui.color_open = true;
        self.ui.edit = Some(Edit::hex(&subtitle_prefs::hex(self.ui.prefs.text_color)));
        window.focus(&self.focus.edit, cx);
        cx.notify();
    }

    /// Feed a key to the open editor. Every key is the editor's while it is open, except Tab.
    pub(super) fn edit_key(
        &mut self,
        key: &str,
        key_char: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(edit) = &mut self.ui.edit else {
            return false;
        };
        let outcome = edit.key(key, key_char);
        let (target, draft) = (edit.target, edit.draft.clone());
        match outcome {
            Outcome::Changed => {
                if target == Target::Hex
                    && let Some(color) = subtitle_prefs::parse_strict_hex(&draft)
                {
                    self.set_color(color, cx);
                }
                cx.notify();
            }
            Outcome::Commit => match target {
                Target::Field(_) => self.commit_edit(window, cx),
                Target::Hex => {
                    if subtitle_prefs::parse_strict_hex(&draft).is_some() {
                        self.cancel_edit(window, cx);
                    }
                }
            },
            Outcome::Cancel => self.cancel_edit(window, cx),
            Outcome::Ignored if key == "tab" => {
                self.commit_edit(window, cx);
                return false;
            }
            Outcome::Ignored => {}
        }
        true
    }

    /// A trigger press: open or close a select's menu.
    pub(super) fn toggle_select(&mut self, id: SelectId, cx: &mut Context<Self>) {
        if self.ui.select_pressed_closed.take() == Some(id) {
            return;
        }
        if self.ui.select.is_some_and(|open| open.id == id) {
            self.ui.select = None;
        } else {
            let (_, current) = select_items(id, &self.ui.output, &self.model.devices);
            self.ui.select = Some(OpenSelect {
                id,
                highlight: current,
            });
        }
        cx.notify();
    }

    /// Choose a menu item and close the menu.
    pub(super) fn choose_select(&mut self, id: SelectId, index: usize, cx: &mut Context<Self>) {
        self.ui.select = None;
        let (items, _) = select_items(id, &self.ui.output, &self.model.devices);
        if let Some(item) = items.get(index)
            && let Some(next) = apply_select(id, &item.value, &self.ui.output)
            && next != self.ui.output
        {
            self.set_output(next, cx);
        }
        cx.notify();
    }

    /// Keys while a menu is open; arrows move, Enter or Space choose.
    pub(super) fn select_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.ui.select else {
            return false;
        };
        let (items, _) = select_items(open.id, &self.ui.output, &self.model.devices);
        let last = items.len().saturating_sub(1);
        let highlight = match key {
            "down" => (open.highlight + 1).min(last),
            "up" => open.highlight.saturating_sub(1),
            "home" => 0,
            "end" => last,
            "enter" | "space" | " " => {
                self.choose_select(open.id, open.highlight, cx);
                return true;
            }
            "tab" => {
                self.ui.select = None;
                cx.notify();
                return false;
            }
            _ => return true,
        };
        self.ui.select = Some(OpenSelect { highlight, ..open });
        cx.notify();
        true
    }

    /// Toggle receiver passthrough for the current device.
    pub(super) fn toggle_passthrough(&mut self, cx: &mut Context<Self>) {
        let device = output::select_devices(&self.model.devices)
            .into_iter()
            .find(|device| device.name == self.ui.output.device);
        if device.is_some_and(|device| device.passthrough_available()) {
            let next = output::with_passthrough(&self.ui.output, !self.ui.output.passthrough());
            self.set_output(next, cx);
        }
    }
}
