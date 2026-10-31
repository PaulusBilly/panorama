//! The rows each player panel shows, derived from playback state.
use super::subtitles;
use crate::{
    film_display::Quality,
    playback::{
        model::{PlaybackModel, Status},
        selection::SourceRef,
        source, torrent_ui,
    },
};
use panorama_core::addons::{StreamGroup, StreamSource, StreamState};

/// Every panel shares one width, so switching panels never resizes the surface.
pub(super) const PANEL_WIDTH: f32 = 320.0;

/// What activating a row does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowKey {
    /// Select an audio track.
    Audio(i64),
    /// Switch to a source.
    Source(SourceRef),
    /// Choose a subtitle language, or Off.
    Language(subtitles::Choice),
    /// Select a subtitle track.
    Subtitle(i64),
}

/// One radio option.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: RowKey,
    pub label: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub selected: bool,
    pub disabled: bool,
    pub multiline: bool,
}

/// A line of caption copy inside a section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub text: String,
    pub danger: bool,
}

/// An addon's group of rows under a sticky label.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub label: Option<String>,
    pub rows: Vec<Row>,
    pub note: Option<Note>,
}

/// One child of a panel's scrolling list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Item {
    /// A section label, by section.
    Label(usize),
    /// An option row, by flat index.
    Row(usize),
    /// A section note, by section.
    Note(usize),
    /// The empty-panel line.
    Empty,
    /// The danger line.
    Error,
}

/// A whole panel page.
#[derive(Clone, Debug, PartialEq)]
pub struct Content {
    pub title: &'static str,
    /// Preferred width in logical pixels, before the window clamp.
    pub width: f32,
    pub sections: Vec<Section>,
    /// Shown when there is nothing to list.
    pub empty: Option<&'static str>,
    /// A danger line under the rows.
    pub error: Option<String>,
}

impl Content {
    /// Every row in display order.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sections.iter().flat_map(|section| section.rows.iter())
    }

    /// Which rows can take focus or selection, in display order.
    pub fn enabled(&self) -> Vec<bool> {
        self.rows().map(|row| !row.disabled).collect()
    }

    /// Index of the selected row.
    pub fn selected(&self) -> Option<usize> {
        self.rows().position(|row| row.selected)
    }

    /// The row at a flat index.
    pub fn row(&self, index: usize) -> Option<&Row> {
        self.rows().nth(index)
    }

    /// The scroll children in display order; the renderer and keyboard scrolling share it.
    pub fn items(&self) -> Vec<Item> {
        let mut items = vec![];
        let mut flat = 0;
        for (index, section) in self.sections.iter().enumerate() {
            if section.label.is_some() {
                items.push(Item::Label(index));
            }
            for _ in &section.rows {
                items.push(Item::Row(flat));
                flat += 1;
            }
            if section.note.is_some() {
                items.push(Item::Note(index));
            }
        }
        if self.empty.is_some() {
            items.push(Item::Empty);
        }
        if self.error.is_some() {
            items.push(Item::Error);
        }
        items
    }
}

/// The Audio panel.
pub fn audio(model: &PlaybackModel) -> Content {
    let rows: Vec<Row> = model
        .audio
        .iter()
        .map(|track| Row {
            key: RowKey::Audio(track.id),
            label: track.label.clone(),
            description: track.description.clone(),
            tags: vec![],
            selected: track.selected,
            disabled: false,
            multiline: false,
        })
        .collect();
    Content {
        title: "Audio",
        width: PANEL_WIDTH,
        empty: rows.is_empty().then_some("No audio tracks"),
        sections: vec![Section {
            label: None,
            rows,
            note: None,
        }],
        error: match model.status {
            Status::Error(error) => Some(error.message().into()),
            _ => None,
        },
    }
}

/// The Subtitles panel's two row sections; the Appearance column has no rows.
pub fn subtitles(view: &subtitles::View) -> Content {
    let row = |key, label: String, description, selected| Row {
        key,
        label,
        description,
        tags: vec![],
        selected,
        disabled: false,
        multiline: false,
    };
    let languages = view
        .languages
        .iter()
        .map(|choice| {
            let label = match choice {
                subtitles::Choice::Off => "Off",
                subtitles::Choice::Lang(lang) => lang.name(),
            };
            row(
                RowKey::Language(*choice),
                label.into(),
                None,
                *choice == view.active,
            )
        })
        .collect();
    let tracks: Vec<Row> = view
        .tracks
        .iter()
        .map(|track| {
            row(
                RowKey::Subtitle(track.id),
                track.label.clone(),
                track.description.clone(),
                view.selected == Some(track.id),
            )
        })
        .collect();
    let note = |text: &str| {
        Some(Note {
            text: text.into(),
            danger: false,
        })
    };
    let track_note = match (view.active, tracks.is_empty()) {
        (subtitles::Choice::Off, _) => note("Subtitles are off."),
        (_, true) => note("No subtitles"),
        _ => None,
    };
    Content {
        title: "Subtitles",
        width: PANEL_WIDTH,
        sections: vec![
            Section {
                label: Some("Language".into()),
                rows: languages,
                note: None,
            },
            Section {
                label: Some("Track".into()),
                rows: tracks,
                note: track_note,
            },
        ],
        empty: None,
        error: None,
    }
}

fn source_row(group: usize, index: usize, stream: &StreamSource, current: SourceRef) -> Row {
    let key = SourceRef {
        group,
        stream: index,
    };
    let selected = key == current;
    let playable = source::resolve(stream).is_ok();
    let quality = Quality::from_source(stream);
    let torrent = torrent_ui::is_torrent(stream);
    let mut tags: Vec<String> = torrent.then(|| "Torrent".to_owned()).into_iter().collect();
    tags.extend(quality.video.map(str::to_owned));
    if quality.surround {
        tags.push("5.1".into());
    }
    let label = stream
        .name
        .clone()
        .or_else(|| stream.title.clone())
        .unwrap_or_else(|| "Source".into());
    Row {
        key: RowKey::Source(key),
        description: if playable {
            let text = stream.title.clone().or_else(|| stream.description.clone());
            // The count moves from the text to its own suffix, so it is not shown twice.
            let (text, mut count) = match text {
                Some(text) if torrent => {
                    let (text, count) = torrent_ui::split_seeders(&text);
                    (Some(text), count)
                }
                text => (text, None),
            };
            if torrent && count.is_none() {
                count = stream
                    .description
                    .as_deref()
                    .and_then(torrent_ui::parse_seeders);
            }
            let description =
                text.filter(|description| !description.is_empty() && *description != label);
            let seeders = count.map(torrent_ui::seeders_label);
            match (description, seeders) {
                (Some(description), Some(seeders)) => Some(format!("{description} · {seeders}")),
                (description, seeders) => description.or(seeders),
            }
        } else {
            Some("Not playable in Panorama".into())
        },
        label,
        tags,
        selected,
        disabled: !selected && !playable,
        multiline: true,
    }
}

/// The Sources panel: one section per addon. Addons with sources lead in account order, then
/// those with none, then those that failed.
pub fn sources(groups: &[StreamGroup], current: SourceRef) -> Content {
    let mut order: Vec<(usize, &StreamGroup)> = groups.iter().enumerate().collect();
    order.sort_by_key(|(_, item)| match &item.state {
        StreamState::Ready(streams) if !streams.is_empty() => 0,
        StreamState::Ready(_) | StreamState::Empty => 1,
        StreamState::Failed(_) => 2,
    });
    let sections = order
        .into_iter()
        .map(|(group, item)| {
            let (rows, note) = match &item.state {
                StreamState::Ready(streams) if !streams.is_empty() => (
                    streams
                        .iter()
                        .enumerate()
                        .map(|(index, stream)| source_row(group, index, stream, current))
                        .collect(),
                    None,
                ),
                StreamState::Ready(_) | StreamState::Empty => (
                    vec![],
                    Some(Note {
                        text: "No sources from this addon.".into(),
                        danger: false,
                    }),
                ),
                StreamState::Failed(_) => (
                    vec![],
                    Some(Note {
                        text: "Sources could not be loaded.".into(),
                        danger: true,
                    }),
                ),
            };
            Section {
                label: Some(item.name.clone()),
                rows,
                note,
            }
        })
        .collect();
    Content {
        title: "Sources",
        width: PANEL_WIDTH,
        empty: groups.is_empty().then_some("No sources available."),
        sections,
        error: None,
    }
}

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;
