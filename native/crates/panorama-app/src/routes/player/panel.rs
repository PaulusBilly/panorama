//! Open, switch and close state of the shared player panel (`usePlayerPanels`).

/// The panels, in trigger order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanelId {
    /// Subtitle language, track and appearance.
    Subtitles,
    /// Audio tracks.
    Audio,
    /// Sources from every stream addon.
    Sources,
    /// Playback settings and Discord sharing.
    Settings,
}

impl PanelId {
    fn order(self) -> i32 {
        self as i32
    }
}

/// What a state change asks the view to animate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Change {
    /// The surface rises out of its trigger.
    Opened,
    /// The surface morphs to another panel; `direction` is 1 toward a later trigger, else -1.
    Switched {
        /// The page leaving.
        from: PanelId,
        /// Slide direction.
        direction: i8,
    },
    /// The surface sinks back.
    Closed,
}

/// Which panel is open and how presses and focus interact with it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Panels {
    active: Option<PanelId>,
    displayed: Option<PanelId>,
    focus_on_open: bool,
    open_at_press: bool,
    pressed_closed: Option<PanelId>,
    /// The keyboard-highlighted option row.
    pub row: Option<usize>,
}

impl Panels {
    /// The open panel; chrome and keys read this.
    pub fn active(&self) -> Option<PanelId> {
        self.active
    }

    /// The panel whose page fills the surface, which outlives `active` until the close animation ends.
    pub fn displayed(&self) -> Option<PanelId> {
        self.displayed
    }

    /// Open a panel, or switch the surface to it.
    pub fn open(&mut self, id: PanelId, keyboard: bool) -> Option<Change> {
        if self.active == Some(id) {
            return None;
        }
        let change = match self.active {
            None => {
                self.focus_on_open = keyboard;
                Change::Opened
            }
            Some(from) => Change::Switched {
                from,
                direction: (id.order() - from.order()).signum() as i8,
            },
        };
        self.active = Some(id);
        self.displayed = Some(id);
        self.row = None;
        Some(change)
    }

    /// A trigger activation. A press that already closed this panel (outside press) stays closed.
    pub fn toggle(&mut self, id: PanelId, keyboard: bool) -> Option<Change> {
        if self.pressed_closed.take() == Some(id) {
            return None;
        }
        if self.active == Some(id) {
            self.close()
        } else {
            self.open(id, keyboard)
        }
    }

    /// Close the open panel.
    pub fn close(&mut self) -> Option<Change> {
        self.active.take().map(|_| Change::Closed)
    }

    /// The close animation finished: the surface unmounts.
    pub fn settle(&mut self) {
        self.displayed = None;
        self.focus_on_open = false;
        self.row = None;
    }

    /// Whether the panel just opened from the keyboard should take focus; true once.
    pub fn take_focus_on_open(&mut self) -> bool {
        std::mem::take(&mut self.focus_on_open)
    }

    /// Every primary press, before click handlers run. An outside press closes the panel.
    pub fn press(&mut self, outside_surface: bool) -> Option<Change> {
        self.open_at_press = self.active.is_some();
        self.pressed_closed = None;
        if outside_surface && let Some(id) = self.active {
            self.pressed_closed = Some(id);
            return self.close();
        }
        None
    }

    /// The click that followed a press has been handled, or a key arrived: an outside press no
    /// longer stands in for a later activation of the same trigger.
    pub fn release_press(&mut self) {
        self.pressed_closed = None;
    }

    /// A press that only dismissed a panel must not also pause the film.
    pub fn swallows_click(&self) -> bool {
        self.open_at_press
    }

    /// Escape: close the panel and report the trigger to refocus when focus sat inside it.
    pub fn escape(&mut self, focus_in_panel: bool) -> Option<Option<PanelId>> {
        let id = self.active?;
        self.close();
        Some(focus_in_panel.then_some(id))
    }
}

/// Radio-group navigation: the next enabled row in `delta` direction, wrapping.
pub fn next_row(current: Option<usize>, delta: i32, enabled: &[bool]) -> Option<usize> {
    let count = enabled.len();
    if !enabled.iter().any(|enabled| *enabled) {
        return None;
    }
    let mut index = current.unwrap_or(if delta >= 0 { count - 1 } else { 0 });
    for _ in 0..count {
        index = (index as i32 + delta.signum()).rem_euclid(count as i32) as usize;
        if enabled[index] {
            return Some(index);
        }
    }
    None
}

/// The row a keyboard-opened panel focuses: the selected one, else the first enabled.
pub fn initial_row(selected: Option<usize>, enabled: &[bool]) -> Option<usize> {
    selected
        .filter(|index| enabled.get(*index).copied().unwrap_or(false))
        .or_else(|| enabled.iter().position(|enabled| *enabled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_switching_and_closing_report_their_motion() {
        let mut panels = Panels::default();
        assert_eq!(panels.open(PanelId::Audio, false), Some(Change::Opened));
        assert_eq!(panels.open(PanelId::Audio, false), None);
        assert_eq!(
            panels.open(PanelId::Sources, false),
            Some(Change::Switched {
                from: PanelId::Audio,
                direction: 1
            })
        );
        assert_eq!(
            panels.open(PanelId::Subtitles, false),
            Some(Change::Switched {
                from: PanelId::Sources,
                direction: -1
            })
        );
        assert_eq!(panels.close(), Some(Change::Closed));
        assert_eq!(
            (panels.active(), panels.displayed()),
            (None, Some(PanelId::Subtitles))
        );
        panels.settle();
        assert_eq!(panels.displayed(), None);
        assert_eq!(panels.close(), None);
    }

    #[test]
    fn only_a_keyboard_opening_takes_focus_and_only_once() {
        let mut panels = Panels::default();
        panels.open(PanelId::Audio, false);
        assert!(!panels.take_focus_on_open());
        panels.close();
        panels.settle();
        panels.open(PanelId::Audio, true);
        panels.open(PanelId::Sources, false);
        assert!(panels.take_focus_on_open());
        assert!(!panels.take_focus_on_open());
    }

    #[test]
    fn an_outside_press_closes_and_its_click_neither_reopens_nor_pauses() {
        let mut panels = Panels::default();
        panels.open(PanelId::Audio, false);
        assert_eq!(panels.press(true), Some(Change::Closed));
        assert!(panels.swallows_click());
        assert_eq!(panels.toggle(PanelId::Audio, false), None);
        assert_eq!(panels.active(), None);
        panels.press(false);
        assert!(!panels.swallows_click());
        assert_eq!(panels.toggle(PanelId::Audio, false), Some(Change::Opened));
    }

    #[test]
    fn a_stale_outside_press_does_not_swallow_a_later_activation() {
        let mut panels = Panels::default();
        panels.open(PanelId::Audio, false);
        panels.press(true);
        panels.release_press();
        assert_eq!(panels.toggle(PanelId::Audio, true), Some(Change::Opened));
    }

    #[test]
    fn presses_inside_the_surface_keep_it_open_and_swallow_the_video_click() {
        let mut panels = Panels::default();
        panels.open(PanelId::Sources, false);
        assert_eq!(panels.press(false), None);
        assert!(panels.swallows_click());
        assert_eq!(panels.active(), Some(PanelId::Sources));
    }

    #[test]
    fn a_press_on_a_different_trigger_switches_after_closing_nothing_else() {
        let mut panels = Panels::default();
        panels.open(PanelId::Audio, false);
        assert_eq!(panels.press(true), Some(Change::Closed));
        assert_eq!(panels.toggle(PanelId::Sources, false), Some(Change::Opened));
    }

    #[test]
    fn escape_closes_first_and_restores_focus_only_from_inside() {
        let mut panels = Panels::default();
        assert_eq!(panels.escape(true), None);
        panels.open(PanelId::Audio, true);
        assert_eq!(panels.escape(true), Some(Some(PanelId::Audio)));
        assert_eq!(panels.active(), None);
        panels.open(PanelId::Sources, false);
        assert_eq!(panels.escape(false), Some(None));
    }

    #[test]
    fn rows_wrap_and_skip_disabled_ones() {
        let enabled = [true, false, true, true];
        assert_eq!(next_row(Some(0), 1, &enabled), Some(2));
        assert_eq!(next_row(Some(3), 1, &enabled), Some(0));
        assert_eq!(next_row(Some(0), -1, &enabled), Some(3));
        assert_eq!(next_row(Some(2), -1, &enabled), Some(0));
        assert_eq!(next_row(None, 1, &enabled), Some(0));
        assert_eq!(next_row(None, -1, &enabled), Some(3));
        assert_eq!(next_row(Some(0), 1, &[false, false]), None);
        assert_eq!(next_row(None, 1, &[]), None);
    }

    #[test]
    fn keyboard_opening_focuses_the_selected_row_else_the_first_enabled() {
        assert_eq!(initial_row(Some(2), &[true, true, true]), Some(2));
        assert_eq!(initial_row(Some(1), &[true, false, true]), Some(0));
        assert_eq!(initial_row(None, &[false, true]), Some(1));
        assert_eq!(initial_row(None, &[false]), None);
    }
}
