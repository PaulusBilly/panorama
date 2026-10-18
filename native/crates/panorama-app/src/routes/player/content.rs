//! The rows each player panel shows, derived from playback state.
use crate::{
    film_display::Quality,
    playback::{
        model::{PlaybackModel, Status},
        selection::SourceRef,
        source,
    },
};
use panorama_core::addons::{StreamGroup, StreamSource, StreamState};

/// What activating a row does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowKey {
    /// Select an audio track.
    Audio(i64),
    /// Switch to a source.
    Source(SourceRef),
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
        width: 288.0,
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

fn source_row(group: usize, index: usize, stream: &StreamSource, current: SourceRef) -> Row {
    let key = SourceRef {
        group,
        stream: index,
    };
    let selected = key == current;
    let playable = source::resolve(stream).is_ok();
    let quality = Quality::from_source(stream);
    let mut tags: Vec<String> = quality.video.map(str::to_owned).into_iter().collect();
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
            stream
                .title
                .clone()
                .or_else(|| stream.description.clone())
                .filter(|description| *description != label)
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

/// The Sources panel: one section per addon, in account order.
pub fn sources(groups: &[StreamGroup], current: SourceRef) -> Content {
    let sections = groups
        .iter()
        .enumerate()
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
        width: 416.0,
        empty: groups.is_empty().then_some("No sources available."),
        sections,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::model::{PlaybackError, Track};
    use panorama_core::addons::{AddonKey, FailureKind};

    fn stream(value: serde_json::Value, name: &str, title: &str) -> StreamSource {
        StreamSource {
            stream: serde_json::from_value(value).expect("fixture stream"),
            name: Some(name.into()),
            title: Some(title.into()),
            description: None,
        }
    }

    fn group(name: &str, state: StreamState) -> StreamGroup {
        StreamGroup {
            addon: serde_json::from_value::<AddonKey>(format!("{name}#0").into())
                .expect("fixture key"),
            name: name.into(),
            state,
        }
    }

    fn fixture() -> Vec<StreamGroup> {
        vec![
            group(
                "Torrentio",
                StreamState::Ready(vec![
                    stream(
                        serde_json::json!({"url": "https://media.invalid/a.mp4"}),
                        "Torrentio 2160p",
                        "2160p DDP5.1",
                    ),
                    stream(
                        serde_json::json!({"infoHash": "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"}),
                        "Torrentio 720p",
                        "720p",
                    ),
                    stream(
                        serde_json::json!({"ytId": "video"}),
                        "Trailer",
                        "YouTube only",
                    ),
                ]),
            ),
            group("Quiet", StreamState::Empty),
            group("Broken", StreamState::Failed(FailureKind::Offline)),
        ]
    }

    #[test]
    fn sources_group_by_addon_in_account_order() {
        let content = sources(&fixture(), SourceRef::default());
        let labels: Vec<_> = content
            .sections
            .iter()
            .map(|section| section.label.as_deref())
            .collect();
        assert_eq!(labels, [Some("Torrentio"), Some("Quiet"), Some("Broken")]);
        assert_eq!(content.sections[0].rows.len(), 3);
        assert_eq!(content.empty, None);
        assert_eq!(
            content.sections[1].note,
            Some(Note {
                text: "No sources from this addon.".into(),
                danger: false
            })
        );
        assert_eq!(
            content.sections[2].note,
            Some(Note {
                text: "Sources could not be loaded.".into(),
                danger: true
            })
        );
    }

    #[test]
    fn a_description_repeating_the_label_is_dropped() {
        let groups = vec![group(
            "Addon",
            StreamState::Ready(vec![stream(
                serde_json::json!({"url": "https://media.invalid/a.mp4"}),
                "Same 1080p",
                "Same 1080p",
            )]),
        )];
        let content = sources(&groups, SourceRef::default());
        assert_eq!(content.sections[0].rows[0].description, None);
        let content = sources(&fixture(), SourceRef::default());
        assert_eq!(
            content.sections[0].rows[0].description.as_deref(),
            Some("2160p DDP5.1")
        );
    }

    #[test]
    fn tags_come_from_the_existing_quality_parser() {
        let content = sources(&fixture(), SourceRef::default());
        assert_eq!(content.sections[0].rows[0].tags, ["4K", "5.1"]);
        assert_eq!(content.sections[0].rows[1].tags, ["HD"]);
        assert!(content.sections[0].rows[2].tags.is_empty());
    }

    #[test]
    fn unplayable_rows_are_disabled_unless_current() {
        let groups = fixture();
        let content = sources(&groups, SourceRef::default());
        let rows = &content.sections[0].rows;
        assert!(!rows[0].disabled && rows[0].selected);
        assert!(!rows[1].disabled && !rows[1].selected);
        assert!(rows[2].disabled);
        assert_eq!(
            rows[2].description.as_deref(),
            Some("Not playable in Panorama")
        );
        assert_eq!(content.enabled(), [true, true, false]);
        assert_eq!(content.selected(), Some(0));
        let current = sources(
            &groups,
            SourceRef {
                group: 0,
                stream: 2,
            },
        );
        assert!(!current.sections[0].rows[2].disabled);
        assert_eq!(current.enabled(), [true, true, true]);
    }

    #[test]
    fn scroll_items_interleave_labels_rows_and_notes_in_display_order() {
        let content = sources(&fixture(), SourceRef::default());
        assert_eq!(
            content.items(),
            [
                Item::Label(0),
                Item::Row(0),
                Item::Row(1),
                Item::Row(2),
                Item::Label(1),
                Item::Note(1),
                Item::Label(2),
                Item::Note(2),
            ]
        );
        assert_eq!(sources(&[], SourceRef::default()).items(), [Item::Empty]);
    }

    #[test]
    fn no_groups_reads_as_no_sources() {
        let content = sources(&[], SourceRef::default());
        assert_eq!(content.empty, Some("No sources available."));
        assert_eq!(content.rows().count(), 0);
    }

    #[test]
    fn audio_rows_mirror_the_track_list() {
        let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
        assert_eq!(audio(&model).empty, Some("No audio tracks"));
        model.audio = vec![
            Track {
                id: 3,
                label: "French".into(),
                description: Some("ac3".into()),
                lang: None,
                codec: None,
                selected: false,
            },
            Track {
                id: 7,
                label: "English".into(),
                description: None,
                lang: None,
                codec: None,
                selected: true,
            },
        ];
        let content = audio(&model);
        assert_eq!(content.empty, None);
        assert_eq!(content.selected(), Some(1));
        assert_eq!(content.row(0).map(|row| row.key), Some(RowKey::Audio(3)));
        assert_eq!(content.error, None);
        model.status = Status::Error(PlaybackError::Decode);
        assert_eq!(
            audio(&model).error.as_deref(),
            Some(PlaybackError::Decode.message())
        );
    }
}
