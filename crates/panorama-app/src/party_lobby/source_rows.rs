//! The host's Source row and its Change list, from the rows the player's Sources panel shows.
use crate::{
    playback::selection::{PlaybackSelection, SourceRef},
    routes::player::content::{self, Row, RowKey},
};

/// The source the party will play, as one line.
#[derive(Clone, Debug, PartialEq)]
pub struct Current {
    /// The release's video quality, such as "4K" or "HD".
    pub quality: Option<String>,
    pub label: String,
    /// The addon, then the release's size and seeders as the addon describes them.
    pub detail: String,
}

/// One addon's choices in the Change list.
#[derive(Clone, Debug, PartialEq)]
pub struct Choices {
    pub addon: String,
    pub rows: Vec<Choice>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub source: SourceRef,
    pub label: String,
    pub detail: Option<String>,
    pub selected: bool,
    pub disabled: bool,
}

pub fn current(selection: &PlaybackSelection) -> Option<Current> {
    let addon = selection.groups.get(selection.source.group)?.name.clone();
    let panel = content::sources(&selection.groups, selection.source);
    let row = panel.rows().find(|row| row.selected)?;
    let detail = [Some(addon), row.description.as_deref().map(one_line)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    Some(Current {
        quality: quality(row),
        label: row.label.clone(),
        detail,
    })
}

/// Every addon with sources to switch to, in the Sources panel's order.
pub fn choices(selection: &PlaybackSelection) -> Vec<Choices> {
    content::sources(&selection.groups, selection.source)
        .sections
        .into_iter()
        .filter(|section| !section.rows.is_empty())
        .map(|section| Choices {
            addon: section.label.unwrap_or_default(),
            rows: section
                .rows
                .iter()
                .filter_map(|row| {
                    let RowKey::Source(source) = row.key else {
                        return None;
                    };
                    Some(Choice {
                        source,
                        label: row.label.clone(),
                        detail: row.description.as_deref().map(one_line),
                        selected: row.selected,
                        disabled: row.disabled,
                    })
                })
                .collect(),
        })
        .collect()
}

fn quality(row: &Row) -> Option<String> {
    row.tags.iter().find(|tag| *tag != "Torrent").cloned()
}

/// Addons stack size, codec and seeders on separate lines; the lobby has one.
fn one_line(text: &str) -> String {
    text.chars()
        .filter(|c| !is_pictograph(*c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Addons decorate sizes and seeders with emoji, which the lobby's type has no place for.
fn is_pictograph(c: char) -> bool {
    matches!(u32::from(c), 0x2600..=0x27BF | 0xFE00..=0xFE0F | 0x1F000..=0x1FAFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(stream: usize) -> PlaybackSelection {
        let film = crate::fixtures::films().remove(0);
        PlaybackSelection {
            video_id: film.id.clone(),
            film,
            groups: crate::fixtures::player_groups(),
            source: SourceRef { group: 0, stream },
            start: crate::playback::selection::StartMode::Restart,
            meta_base: None,
            subtitles: Vec::new(),
            late_subtitles: None,
        }
    }

    #[test]
    fn the_source_row_names_the_addon_and_the_release() {
        let current = current(&selection(1)).expect("selected source");
        assert_eq!(current.label, "Fixture 1080p");
        assert!(current.detail.starts_with("Fixture Streams · "));
        assert!(!current.detail.contains('\n'));
    }

    #[test]
    fn an_addons_emoji_stay_out_of_the_detail() {
        assert_eq!(one_line("💾 4.2 GB\n👤 128"), "4.2 GB 128");
    }

    #[test]
    fn change_lists_only_addons_with_sources() {
        let choices = choices(&selection(1));
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].addon, "Fixture Streams");
        assert_eq!(choices[0].rows.iter().filter(|row| row.selected).count(), 1);
        assert!(choices[0].rows[1].selected);
    }
}
