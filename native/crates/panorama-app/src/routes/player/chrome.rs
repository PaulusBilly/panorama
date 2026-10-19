//! When the player chrome shows, ported from `PlayerDialog.tsx`.
use std::time::{Duration, Instant};

/// Idle time before the chrome fades.
pub const HIDE_DELAY: Duration = Duration::from_millis(3000);

/// What the chrome reads from the player each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    /// Status is `Ready` (not buffering, preparing, ended or failed).
    pub ready: bool,
    /// Ready or Buffering: the controls exist.
    pub controls_ready: bool,
    /// Initial preparation owns the whole frame.
    pub initial_preparing: bool,
    /// A control has focus after keyboard interaction.
    pub keyboard_focus: bool,
    /// A panel is open.
    pub panel_open: bool,
    /// Playback is paused.
    pub paused: bool,
    /// A scrub or volume drag is in progress.
    pub dragging: bool,
}

/// Keyboard focus pins the chrome; focus from a pointer press does not.
pub fn keyboard_focus(keyboard_navigation: bool, control_focused: bool) -> bool {
    keyboard_navigation && control_focused
}

/// The resolved visibility of each chrome part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    /// The top bar (close, volume, fullscreen).
    pub top: bool,
    /// The bottom bar (title, panels, seek).
    pub bottom: bool,
    /// Whether the pointer shows over the video.
    pub pointer: bool,
}

/// Activity clock for the auto-hiding chrome.
#[derive(Clone, Copy, Debug)]
pub struct Chrome {
    activity: Instant,
    pinned: bool,
}

impl Chrome {
    /// Start visible with the hide timer at `now`.
    pub fn new(now: Instant) -> Self {
        Self {
            activity: now,
            pinned: false,
        }
    }

    /// A pointer move or press, a shortcut, or a fullscreen change restarts the timer.
    pub fn activity(&mut self, now: Instant) {
        self.activity = now;
    }

    fn pinned(inputs: Inputs) -> bool {
        !inputs.ready
            || inputs.keyboard_focus
            || inputs.panel_open
            || inputs.paused
            || inputs.dragging
    }

    /// Resolve visibility; the timer restarts when the last pin releases.
    pub fn shown(&mut self, now: Instant, inputs: Inputs) -> Shown {
        let pinned = Self::pinned(inputs);
        if self.pinned && !pinned {
            self.activity = now;
        }
        self.pinned = pinned;
        let recent = now.saturating_duration_since(self.activity) < HIDE_DELAY;
        let interaction = recent || pinned;
        let top = !inputs.initial_preparing && (!inputs.ready || interaction);
        Shown {
            top,
            bottom: !inputs.initial_preparing && inputs.controls_ready && interaction,
            pointer: inputs.initial_preparing || top,
        }
    }

    /// How long until the timer hides the chrome, when it is running.
    pub fn deadline(&self, now: Instant) -> Option<Duration> {
        (!self.pinned)
            .then(|| HIDE_DELAY.checked_sub(now.saturating_duration_since(self.activity)))
            .flatten()
            .filter(|left| !left.is_zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const READY: Inputs = Inputs {
        ready: true,
        controls_ready: true,
        initial_preparing: false,
        keyboard_focus: false,
        panel_open: false,
        paused: false,
        dragging: false,
    };

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn idle_chrome_fades_after_three_seconds() {
        let start = Instant::now();
        let mut chrome = Chrome::new(start);
        assert!(chrome.shown(at(start, 2999), READY).bottom);
        let hidden = chrome.shown(at(start, 3000), READY);
        assert_eq!(
            hidden,
            Shown {
                top: false,
                bottom: false,
                pointer: false
            }
        );
    }

    #[test]
    fn pointer_activity_restarts_the_timer() {
        let start = Instant::now();
        let mut chrome = Chrome::new(start);
        chrome.activity(at(start, 2500));
        assert!(chrome.shown(at(start, 5000), READY).top);
        assert!(!chrome.shown(at(start, 5500), READY).top);
        assert_eq!(chrome.deadline(at(start, 5500)), None);
        chrome.activity(at(start, 6000));
        assert_eq!(
            chrome.deadline(at(start, 6500)),
            Some(Duration::from_millis(2500))
        );
    }

    #[test]
    fn focus_panel_and_pause_pin_then_restart_the_timer_on_release() {
        for pin in [
            Inputs {
                keyboard_focus: true,
                ..READY
            },
            Inputs {
                panel_open: true,
                ..READY
            },
            Inputs {
                paused: true,
                ..READY
            },
        ] {
            let start = Instant::now();
            let mut chrome = Chrome::new(start);
            assert!(chrome.shown(at(start, 10_000), pin).bottom);
            assert_eq!(chrome.deadline(at(start, 10_000)), None);
            assert!(chrome.shown(at(start, 12_000), READY).bottom);
            assert!(!chrome.shown(at(start, 15_000), READY).bottom);
        }
    }

    #[test]
    fn a_drag_pins_the_bar_until_it_ends() {
        let start = Instant::now();
        let mut chrome = Chrome::new(start);
        let drag = Inputs {
            dragging: true,
            ..READY
        };
        assert!(chrome.shown(at(start, 20_000), drag).bottom);
        assert!(chrome.shown(at(start, 22_000), READY).bottom);
        assert!(!chrome.shown(at(start, 25_000), READY).bottom);
    }

    #[test]
    fn only_keyboard_focus_pins() {
        assert!(keyboard_focus(true, true));
        assert!(!keyboard_focus(false, true));
        assert!(!keyboard_focus(true, false));
    }

    #[test]
    fn buffering_keeps_the_top_bar_and_the_controls() {
        let start = Instant::now();
        let mut chrome = Chrome::new(start);
        let buffering = Inputs {
            ready: false,
            ..READY
        };
        let shown = chrome.shown(at(start, 60_000), buffering);
        assert!(shown.top && shown.bottom && shown.pointer);
    }

    #[test]
    fn end_and_error_keep_the_top_bar_without_controls() {
        let start = Instant::now();
        let mut chrome = Chrome::new(start);
        let ended = Inputs {
            ready: false,
            controls_ready: false,
            ..READY
        };
        let shown = chrome.shown(at(start, 60_000), ended);
        assert!(shown.top && !shown.bottom && shown.pointer);
    }

    #[test]
    fn initial_preparing_shows_only_the_pointer() {
        let start = Instant::now();
        let mut chrome = Chrome::new(start);
        let preparing = Inputs {
            ready: false,
            controls_ready: false,
            initial_preparing: true,
            ..READY
        };
        let shown = chrome.shown(start, preparing);
        assert!(!shown.top && !shown.bottom && shown.pointer);
    }
}
