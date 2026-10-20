use super::range::RangeId;
use crate::playback::{host::Command, model::PlaybackModel};
use gpui::KeyDownEvent;

/// What the shell does after offering a key to the player.
pub enum KeyAction {
    /// Leave the player for the film page.
    Close,
    /// The player used the key.
    Handled,
}

/// A global shortcut, active when focus is not inside a range or the panel.
pub enum Shortcut {
    Command(Command),
    Fullscreen,
}

/// Whether the key carries a modifier that belongs to the system, not the player.
pub fn reserved(event: &KeyDownEvent) -> bool {
    let modifiers = &event.keystroke.modifiers;
    modifiers.control || modifiers.platform || modifiers.alt
}

/// Where keyboard focus rests, for routing a key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusZone {
    /// Nothing in the player has focus.
    Nothing,
    /// A button: a chrome control, an overlay button or a notice button.
    Button,
    /// A range.
    Range(RangeId),
    /// Inside the open panel.
    Panel,
}

/// Who handles a key after Escape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Route {
    /// The panel's own navigation.
    Panel,
    /// The focused range.
    Range(RangeId),
    /// The focused button activates itself on Space and Enter.
    Button,
    /// The global shortcuts.
    Shortcut,
}

/// Route a key by focus. Space and Enter belong to a focused button, not to pause.
pub fn route(key: &str, zone: FocusZone, panel_open: bool) -> Route {
    match zone {
        FocusZone::Panel if panel_open => Route::Panel,
        FocusZone::Range(id) => Route::Range(id),
        FocusZone::Button if matches!(key, "space" | " " | "enter") => Route::Button,
        _ => Route::Shortcut,
    }
}

/// The shortcut for a key, mirroring `handleShortcut` in `PlayerDialog.tsx`.
pub fn shortcut(key: &str, model: &PlaybackModel) -> Option<Shortcut> {
    let command = match key {
        "space" | " " | "k" => Command::TogglePause,
        "left" => Command::SeekRelative(-10.0),
        "right" => Command::SeekRelative(10.0),
        "m" => Command::SetMuted(!model.muted),
        "up" => Command::SetVolume(model.volume.saturating_add(10).min(100)),
        "down" => Command::SetVolume(model.volume.saturating_sub(10)),
        "f" => return Some(Shortcut::Fullscreen),
        _ => return None,
    };
    Some(Shortcut::Command(command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::selection::SourceRef;

    fn command(key: &str, model: &PlaybackModel) -> Option<String> {
        match shortcut(key, model)? {
            Shortcut::Command(Command::TogglePause) => Some("pause".into()),
            Shortcut::Command(Command::SeekRelative(seconds)) => Some(format!("seek {seconds}")),
            Shortcut::Command(Command::SetMuted(muted)) => Some(format!("mute {muted}")),
            Shortcut::Command(Command::SetVolume(volume)) => Some(format!("volume {volume}")),
            Shortcut::Fullscreen => Some("fullscreen".into()),
            Shortcut::Command(_) => None,
        }
    }

    #[test]
    fn space_and_enter_go_to_a_focused_button_not_to_pause() {
        use FocusZone::*;
        for key in ["space", " ", "enter"] {
            assert_eq!(route(key, Button, false), Route::Button, "{key}");
            assert_eq!(route(key, Button, true), Route::Button, "{key}");
            assert_eq!(route(key, Nothing, false), Route::Shortcut, "{key}");
            assert_eq!(route(key, Panel, true), Route::Panel, "{key}");
            assert_eq!(
                route(key, Range(RangeId::Seek), false),
                Route::Range(RangeId::Seek)
            );
        }
        assert_eq!(route("m", Button, false), Route::Shortcut);
        assert_eq!(route("space", Panel, false), Route::Shortcut);
    }

    #[test]
    fn shortcuts() {
        let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
        model.volume = 95;
        for (key, expected) in [
            ("space", "pause"),
            ("k", "pause"),
            ("left", "seek -10"),
            ("right", "seek 10"),
            ("m", "mute true"),
            ("up", "volume 100"),
            ("down", "volume 85"),
            ("f", "fullscreen"),
        ] {
            assert_eq!(command(key, &model).as_deref(), Some(expected), "{key}");
        }
        assert_eq!(command("x", &model), None);
        model.muted = true;
        assert_eq!(command("m", &model).as_deref(), Some("mute false"));
        model.volume = 3;
        assert_eq!(command("down", &model).as_deref(), Some("volume 0"));
    }
}
