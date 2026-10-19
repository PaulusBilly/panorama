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
        addon: serde_json::from_value::<AddonKey>(format!("{name}#0").into()).expect("fixture key"),
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
    assert_eq!(content.sections[0].rows[1].tags, ["Torrent", "HD"]);
    assert!(content.sections[0].rows[2].tags.is_empty());
}

#[test]
fn torrent_rows_are_tagged_first_and_carry_the_seeders_count() {
    let hash = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
    let groups = vec![group(
        "Addon",
        StreamState::Ready(vec![
            stream(
                serde_json::json!({"infoHash": hash}),
                "Torrentio 4K",
                "Movie 2160p
👤 42 💾 9 GB",
            ),
            stream(
                serde_json::json!({"infoHash": hash}),
                "Torrentio",
                "Movie
Seeders: 1",
            ),
            stream(
                serde_json::json!({"infoHash": hash}),
                "Torrentio",
                "Movie 1080p",
            ),
            stream(
                serde_json::json!({"url": "https://media.invalid/a.mp4"}),
                "Direct",
                "Movie 👤 99",
            ),
        ]),
    )];
    let rows = &sources(&groups, SourceRef::default()).sections[0].rows;
    assert_eq!(rows[0].tags, ["Torrent", "4K"]);
    assert_eq!(
        rows[0].description.as_deref(),
        Some(
            "Movie 2160p
💾 9 GB · 42 seeders"
        )
    );
    assert_eq!(rows[1].description.as_deref(), Some("Movie · 1 seeder"));
    assert_eq!(rows[2].tags, ["Torrent", "HD"]);
    assert_eq!(rows[2].description.as_deref(), Some("Movie 1080p"));
    // Only torrents get the tag or the count.
    assert!(rows[3].tags.is_empty());
    assert_eq!(rows[3].description.as_deref(), Some("Movie 👤 99"));
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
fn subtitle_rows_follow_the_view_and_explain_empty_columns() {
    use subtitles::{Choice, Lang, State};
    let track = |id: i64, lang: &str, label: &str, selected: bool| Track {
        id,
        label: label.into(),
        description: None,
        lang: Some(lang.into()),
        codec: Some("subrip".into()),
        selected,
    };
    let mut tracks = vec![
        track(1, "eng", "English", false),
        track(2, "ind", "Indonesian", false),
    ];
    let off = subtitles(&State::default().view(&tracks));
    assert_eq!(off.title, "Subtitles");
    assert_eq!(off.sections[0].rows.len(), 3);
    assert!(off.sections[0].rows[0].selected, "Off is highlighted");
    assert_eq!(
        off.sections[1].note.as_ref().map(|note| note.text.as_str()),
        Some("Subtitles are off.")
    );
    tracks[1].selected = true;
    let on = subtitles(&State::default().view(&tracks));
    assert_eq!(
        on.sections[0].rows[2].key,
        RowKey::Language(Choice::Lang(Lang::Id))
    );
    assert!(on.sections[0].rows[2].selected);
    assert_eq!(on.sections[1].rows.len(), 1);
    assert_eq!(on.sections[1].rows[0].key, RowKey::Subtitle(2));
    assert!(on.sections[1].rows[0].selected);
    assert_eq!(on.sections[1].note, None);
    // The flat row order is languages, then tracks (keyboard rows share it).
    assert_eq!(on.selected(), Some(2));
    let none = subtitles(&State::default().view(&[]));
    assert_eq!(none.sections[0].rows.len(), 1);
    let mut chosen = State::default();
    chosen.choose(Choice::Lang(Lang::En), &[track(9, "fra", "French", false)]);
    let empty = subtitles(&chosen.view(&[track(9, "fra", "French", false)]));
    assert_eq!(
        empty.sections[1]
            .note
            .as_ref()
            .map(|note| note.text.as_str()),
        Some("No subtitles")
    );
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
