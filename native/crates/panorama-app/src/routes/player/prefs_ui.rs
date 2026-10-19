//! Preference state the subtitle and settings panels edit.
use super::{edit::Edit, subtitles};
use crate::playback::{
    output::{self, Channels, Device, PlaybackSettings, Video},
    prefs::DiscordView,
    subtitle_prefs::SubtitlePreferences,
    subtitle_render::Renderer,
};
use gpui::Task;

/// The three selects of the settings panel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectId {
    /// Audio output device.
    Device,
    /// Decoded channels.
    Channels,
    /// Video output.
    Video,
}

impl SelectId {
    /// Position in [`Self::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// The field label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Device => "Audio output",
            Self::Channels => "Decoded channels",
            Self::Video => "Video output",
        }
    }
}

/// An open select menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenSelect {
    /// Which select.
    pub id: SelectId,
    /// The keyboard-highlighted item.
    pub highlight: usize,
}

/// One item of a select.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectItem {
    /// The stored value.
    pub value: String,
    /// The label.
    pub label: String,
}

/// The items of a select and the index of the current one.
pub fn select_items(
    id: SelectId,
    settings: &PlaybackSettings,
    devices: &[Device],
) -> (Vec<SelectItem>, usize) {
    let (items, current): (Vec<SelectItem>, String) = match id {
        SelectId::Device => (
            output::select_devices(devices)
                .into_iter()
                .map(|device| SelectItem {
                    value: device.name,
                    label: device.label,
                })
                .collect(),
            settings.device.clone(),
        ),
        SelectId::Channels => (
            Channels::ALL
                .map(|item| SelectItem {
                    value: item.name().into(),
                    label: item.label().into(),
                })
                .to_vec(),
            settings.channels.name().into(),
        ),
        SelectId::Video => (
            Video::ALL
                .map(|item| SelectItem {
                    value: item.name().into(),
                    label: item.label().into(),
                })
                .to_vec(),
            settings.video.name().into(),
        ),
    };
    let index = items
        .iter()
        .position(|item| item.value == current)
        .unwrap_or(0);
    (items, index)
}

/// Apply a chosen select value.
pub fn apply_select(
    id: SelectId,
    value: &str,
    settings: &PlaybackSettings,
) -> Option<PlaybackSettings> {
    let mut next = settings.clone();
    match id {
        SelectId::Device => next = output::with_device(settings, value),
        SelectId::Channels => {
            next.channels = Channels::ALL.into_iter().find(|c| c.name() == value)?
        }
        SelectId::Video => next.video = Video::ALL.into_iter().find(|v| v.name() == value)?,
    }
    Some(next)
}

/// Everything the panels hold between frames.
pub struct PrefsUi {
    /// Language and track memory.
    pub subs: subtitles::State,
    /// The live subtitle preferences.
    pub prefs: SubtitlePreferences,
    /// The live output settings.
    pub output: PlaybackSettings,
    /// The status line's notice.
    pub notice: Option<&'static str>,
    /// Discord sharing, once read.
    pub discord: Option<DiscordView>,
    /// A stepper or the hex input being edited.
    pub edit: Option<Edit>,
    /// The open select menu.
    pub select: Option<OpenSelect>,
    /// A select whose trigger press just closed its menu.
    pub select_pressed_closed: Option<SelectId>,
    /// The custom colour popover is open.
    pub color_open: bool,
    /// A capture override is active: nothing is written to the store.
    pub ephemeral: bool,
    /// Stored preferences have been read.
    pub loaded: bool,
    /// What mpv was last told for the subtitles.
    pub applied: Option<(SubtitlePreferences, Renderer)>,
    /// Debounced writes: subtitle preferences, then playback settings.
    pub saves: [Option<Task<()>>; 2],
    /// Changes not yet written, flushed when the player closes.
    pub dirty: [bool; 2],
}

impl PrefsUi {
    /// Defaults until the stored values arrive.
    pub fn new() -> Self {
        Self {
            subs: subtitles::State::default(),
            prefs: SubtitlePreferences::default(),
            output: PlaybackSettings::default(),
            notice: None,
            discord: None,
            edit: None,
            select: None,
            select_pressed_closed: None,
            color_open: false,
            ephemeral: false,
            loaded: false,
            applied: None,
            saves: [None, None],
            dirty: [false; 2],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_items_mark_the_current_value() {
        let settings = PlaybackSettings {
            channels: Channels::Surround51,
            video: Video::Sdr,
            ..Default::default()
        };
        let (items, current) = select_items(SelectId::Channels, &settings, &[]);
        let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["Auto", "Stereo", "5.1", "7.1"]);
        assert_eq!(current, 2);
        let (items, current) = select_items(SelectId::Video, &settings, &[]);
        assert_eq!((items.len(), current), (2, 1));
        let (items, current) = select_items(SelectId::Device, &settings, &[]);
        assert_eq!((items[0].label.as_str(), current), ("System default", 0));
    }

    #[test]
    fn choosing_applies_to_the_right_setting() {
        let settings = PlaybackSettings::default();
        assert_eq!(
            apply_select(SelectId::Channels, "7.1", &settings).map(|s| s.channels),
            Some(Channels::Surround71)
        );
        assert_eq!(
            apply_select(SelectId::Video, "sdr", &settings).map(|s| s.video),
            Some(Video::Sdr)
        );
        assert_eq!(
            apply_select(SelectId::Device, "wasapi/x", &settings).map(|s| s.device),
            Some("wasapi/x".to_owned())
        );
        assert_eq!(apply_select(SelectId::Video, "hdr", &settings), None);
    }
}
