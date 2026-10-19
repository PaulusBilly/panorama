use crate::playback::{host::Command, model::PlaybackModel};
use gpui::KeyDownEvent;

pub enum KeyAction {
    Close,
    Command(Command),
}

pub fn action(event: &KeyDownEvent, model: Option<&PlaybackModel>) -> Option<KeyAction> {
    let modifiers = &event.keystroke.modifiers;
    if modifiers.control || modifiers.platform || modifiers.alt {
        return None;
    }
    let command = match event.keystroke.key.as_str() {
        "escape" => return Some(KeyAction::Close),
        "space" | " " | "k" => Command::TogglePause,
        "left" => Command::SeekRelative(-10.0),
        "right" => Command::SeekRelative(10.0),
        "m" => Command::SetMuted(!model.is_some_and(|model| model.muted)),
        "up" => Command::SetVolume(
            model
                .map_or(100, |model| model.volume)
                .saturating_add(10)
                .min(100),
        ),
        "down" => Command::SetVolume(model.map_or(100, |model| model.volume).saturating_sub(10)),
        _ => return None,
    };
    Some(KeyAction::Command(command))
}
