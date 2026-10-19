//! Hides the pointer while the player is idle.
//!
//! GPUI offers no public call for hiding the pointer, but `Window::dispatch_key_event` calls the
//! platform's `hide_cursor_until_mouse_moves` for any `KeyDownEvent` whose keystroke has a
//! `key_char`, while `App::cursor_hide_mode` is `OnTyping` or `OnTypingAndAction` (the default).
//! On Windows the platform sets a flag and calls `SetCursor(None)` when the window is hovered,
//! and `handle_mouse_move_msg` restores it through `restore_cursor_after_hide`. So the player
//! dispatches a sentinel key through `Window::dispatch_event`; the shell consumes it before
//! anything reacts (see [`is_sentinel`]).
use super::{Player, chrome};
use gpui::{
    App, Context, CursorHideMode, KeyDownEvent, Keystroke, Modifiers, Pixels, PlatformInput, Point,
    Window,
};

/// The key name of the sentinel keystroke. No keyboard produces it with a `key_char`.
pub const SENTINEL_KEY: &str = "f24";

fn sentinel() -> Keystroke {
    Keystroke {
        key: SENTINEL_KEY.into(),
        key_char: Some(String::new()),
        modifiers: Modifiers::default(),
    }
}

/// Whether this key is the pointer-hide sentinel: `f24` with an empty `key_char` and no
/// modifiers. A real `f24` press carries no `key_char`.
pub fn is_sentinel(event: &KeyDownEvent) -> bool {
    let keystroke = &event.keystroke;
    keystroke.key == SENTINEL_KEY
        && keystroke.key_char.as_deref() == Some("")
        && keystroke.modifiers == Modifiers::default()
}

/// Whether GPUI's hide policy lets a character key hide the pointer.
pub fn mode_allows_hide(mode: CursorHideMode) -> bool {
    matches!(
        mode,
        CursorHideMode::OnTyping | CursorHideMode::OnTypingAndAction
    )
}

/// Hide the pointer until the next mouse move. It does nothing under `CursorHideMode::Never`.
pub fn hide_until_moved(window: &mut Window, cx: &mut App) {
    debug_assert!(
        mode_allows_hide(cx.cursor_hide_mode()),
        "the idle pointer hide needs CursorHideMode::OnTyping or OnTypingAndAction"
    );
    window.dispatch_event(
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: sentinel(),
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
}

impl Player {
    /// Hide the pointer once when the chrome's pointer flag falls while the pointer is inside
    /// the window. The dispatch is deferred out of the draw.
    pub(super) fn sync_pointer(
        &mut self,
        shown: chrome::Shown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.mounted
            || !self
                .pointer
                .update(shown.pointer, window.is_window_hovered())
        {
            return;
        }
        self.defer_hide(window, cx);
    }

    /// Hide the pointer again after the platform restored it without a real move.
    pub(super) fn rehide_pointer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mounted {
            self.defer_hide(window, cx);
        }
    }

    fn defer_hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            // Dispatching a key sets the keyboard modality: a control focused by a pointer press
            // would show a ring, so release it first.
            let _ = weak.update(cx, |player, cx| {
                if player.focus.chrome_focused(window) && !player.keyboard_active {
                    window.blur(cx);
                }
            });
            hide_until_moved(window, cx);
        });
    }
}

/// What a pointer move means for the idle hide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveAction {
    /// A real move: wake the chrome.
    Wake,
    /// A same-position move after the hide fired: the platform restored the pointer, so hide it
    /// again. The chrome stays as it is.
    Rehide,
    /// A same-position move while the pointer is shown: nothing to do.
    Ignore,
}

/// Edge trigger and move filter for the pointer hide.
#[derive(Clone, Copy, Debug)]
pub struct Hider {
    armed: bool,
    last: Option<Point<Pixels>>,
}

impl Default for Hider {
    fn default() -> Self {
        Self {
            armed: true,
            last: None,
        }
    }
}

impl Hider {
    /// Feed the resolved `Shown.pointer` and whether the window holds the pointer; true once per
    /// shown to hidden edge, when the hide should be dispatched. Showing again re-arms it.
    pub fn update(&mut self, pointer_shown: bool, hovered: bool) -> bool {
        if pointer_shown {
            self.armed = true;
            return false;
        }
        let fire = self.armed && hovered;
        if fire {
            self.armed = false;
        }
        fire
    }

    /// Classify a pointer move. Windows can send a move without movement; that must not wake
    /// the chrome, and GPUI restores the pointer on it, so it is hidden again when the hide has
    /// already fired for this idle period.
    pub fn on_move(&mut self, position: Point<Pixels>) -> MoveAction {
        if self.last != Some(position) {
            self.last = Some(position);
            MoveAction::Wake
        } else if self.armed {
            MoveAction::Ignore
        } else {
            MoveAction::Rehide
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};

    fn key(key: &str, key_char: Option<&str>) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke {
                key: key.into(),
                key_char: key_char.map(Into::into),
                modifiers: Modifiers::default(),
            },
            is_held: false,
            prefer_character_input: false,
        }
    }

    #[test]
    fn the_sentinel_is_recognized_and_a_real_f24_is_not() {
        let own = KeyDownEvent {
            keystroke: sentinel(),
            is_held: false,
            prefer_character_input: false,
        };
        assert!(is_sentinel(&own));
        assert!(!is_sentinel(&key("f24", None)));
        assert!(!is_sentinel(&key("f24", Some("x"))));
        assert!(!is_sentinel(&key("f23", Some(""))));
        let mut shifted = own;
        shifted.keystroke.modifiers.shift = true;
        assert!(!is_sentinel(&shifted));
    }

    #[test]
    fn the_sentinel_has_a_key_char_so_gpui_hides_the_pointer() {
        assert!(sentinel().key_char.is_some());
    }

    #[test]
    fn the_hide_depends_on_gpuis_default_mode() {
        assert!(mode_allows_hide(CursorHideMode::default()));
        assert!(mode_allows_hide(CursorHideMode::OnTyping));
        assert!(!mode_allows_hide(CursorHideMode::Never));
    }

    #[test]
    fn hiding_fires_once_per_edge_and_rearms_when_shown() {
        let mut hider = Hider::default();
        assert!(!hider.update(true, true));
        assert!(hider.update(false, true));
        assert!(!hider.update(false, true));
        assert!(!hider.update(false, true));
        assert!(!hider.update(true, true));
        assert!(hider.update(false, true));
    }

    #[test]
    fn hiding_waits_for_the_pointer_to_be_inside_the_window() {
        let mut hider = Hider::default();
        assert!(!hider.update(false, false));
        assert!(hider.update(false, true));
    }

    #[test]
    fn a_pinned_chrome_never_fires() {
        let mut hider = Hider::default();
        for _ in 0..10 {
            assert!(!hider.update(true, true));
        }
    }

    #[test]
    fn a_same_position_move_rehides_only_while_hidden() {
        let mut hider = Hider::default();
        let here = point(px(10.0), px(20.0));
        assert_eq!(hider.on_move(here), MoveAction::Wake);
        assert_eq!(hider.on_move(here), MoveAction::Ignore);
        assert!(hider.update(false, true));
        assert_eq!(hider.on_move(here), MoveAction::Rehide);
        assert_eq!(hider.on_move(here), MoveAction::Rehide);
        assert_eq!(hider.on_move(point(px(11.0), px(20.0))), MoveAction::Wake);
        assert!(!hider.update(true, true));
        assert_eq!(hider.on_move(point(px(11.0), px(20.0))), MoveAction::Ignore);
    }
}
