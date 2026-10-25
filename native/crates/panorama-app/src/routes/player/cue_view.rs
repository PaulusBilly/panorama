//! The cue GPUI draws over the video for text subtitle tracks.
use super::{Player, measure::probe, panel::PanelId, subtitles::Tab, subtitles_view::styled_cue};

/// The stand-in cue: two lines, so line height and padding show too.
const PREVIEW: &str = "This is how your subtitles\nwill look in the film.";
use crate::playback::subtitle_render::{Appearance, bottom_inset, draws_cue};
use gpui::{AnyElement, div, prelude::*, px};

impl Player {
    /// The current cue, centred at the preferred height; `None` when mpv renders the track,
    /// nothing is showing or the preferences have not loaded. While the Appearance tab is open
    /// a stand-in line shows in its place, so every change previews on the film itself.
    pub(super) fn cue_overlay(&self) -> Option<AnyElement> {
        let selected = self
            .model
            .subtitles
            .iter()
            .find(|track| track.selected)
            .map(|track| track.codec.as_deref());
        if !self.ui.loaded {
            return None;
        }
        let previewing = self.panels.active() == Some(PanelId::Subtitles)
            && self.ui.subs.tab == Tab::Settings
            && self.renderer().limitation().is_none();
        let cue = self.model.cue.as_deref().filter(|_| draws_cue(selected));
        let Some(text) = cue.or(previewing.then_some(PREVIEW)) else {
            self.measures.cue.set(None);
            return None;
        };
        let height = f32::from(self.measures.root.get().size.height);
        let box_height = self
            .measures
            .cue
            .get()
            .map_or(0.0, |size| f32::from(size.height));
        let bottom = bottom_inset(height, box_height, self.ui.prefs.vertical_position as f32);
        let look = Appearance::from(&self.ui.prefs);
        Some(
            div()
                .absolute()
                .left(px(16.0))
                .right(px(16.0))
                .bottom(px(bottom))
                .flex()
                .justify_center()
                .child(styled_cue(text, &look).relative().child(probe(
                    &self.measures,
                    |measures, bounds| {
                        measures.cue.set(Some(bounds.size));
                    },
                )))
                .into_any_element(),
        )
    }
}
