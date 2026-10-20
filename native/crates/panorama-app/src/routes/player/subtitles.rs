//! Language and track choices of the Subtitles panel (`PlayerSubtitlesPopover.tsx`).
use crate::playback::model::Track;
use std::collections::HashMap;

/// The languages the panel offers (`SubtitleLanguageId`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Lang {
    /// English.
    En,
    /// Indonesian.
    Id,
}

impl Lang {
    /// `subtitleLanguageName`.
    pub fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Id => "Indonesian",
        }
    }
}

/// The language column's choices.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Choice {
    /// No subtitles.
    Off,
    /// A language.
    Lang(Lang),
}

/// The narrow layout's tabs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Tab {
    /// Language.
    #[default]
    Languages,
    /// Track.
    Variants,
    /// Appearance.
    Settings,
}

impl Tab {
    /// Every tab in display order with its label.
    pub const ALL: [(Self, &'static str); 3] = [
        (Self::Languages, "Language"),
        (Self::Variants, "Track"),
        (Self::Settings, "Appearance"),
    ];
}

fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Normalized language identity of a subtitle track.
pub fn language_id(language: Option<&str>, label: Option<&str>) -> Option<Lang> {
    let code = language
        .unwrap_or("")
        .trim()
        .to_lowercase()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_owned();
    match code.as_str() {
        "en" | "eng" => return Some(Lang::En),
        "id" | "ind" => return Some(Lang::Id),
        _ => {}
    }
    let text = format!("{} {}", language.unwrap_or(""), label.unwrap_or("")).to_lowercase();
    if has_word(&text, "indonesian") || has_word(&text, "bahasa") {
        Some(Lang::Id)
    } else if has_word(&text, "english") {
        Some(Lang::En)
    } else {
        None
    }
}

fn track_lang(track: &Track) -> Option<Lang> {
    language_id(track.lang.as_deref(), Some(&track.label))
}

/// A row of the Track column.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackRow {
    /// mpv track id.
    pub id: i64,
    /// The source label, numbered when repeated.
    pub label: String,
    /// The track's own label when it differs from the language name.
    pub description: Option<String>,
    /// Whether mpv has it selected.
    pub selected: bool,
}

/// What the three columns show.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// The language rows, "Off" first.
    pub languages: Vec<Choice>,
    /// The highlighted language.
    pub active: Choice,
    /// The active language's tracks.
    pub tracks: Vec<TrackRow>,
    /// The selected track's id.
    pub selected: Option<i64>,
}

/// `embeddedSubtitleSourceLabel`.
fn source_label(index: usize, codec: Option<&str>) -> String {
    match codec {
        Some(codec) if !codec.is_empty() => format!("Embedded {} \u{b7} {codec}", index + 1),
        _ => format!("Embedded {}", index + 1),
    }
}

/// Repeated source labels get a " · N" suffix so the rows stay distinguishable (addon tracks
/// often share the addon's name).
pub fn number_repeats(labels: Vec<String>) -> Vec<String> {
    labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            if labels.iter().filter(|other| *other == label).count() > 1 {
                let seen = labels[..=index]
                    .iter()
                    .filter(|other| *other == label)
                    .count();
                format!("{label} \u{b7} {seen}")
            } else {
                label.clone()
            }
        })
        .collect()
}

/// Session memory of the panel.
#[derive(Clone, Debug, Default)]
pub struct State {
    choice: Option<Choice>,
    remembered: HashMap<Lang, i64>,
    /// The narrow layout's tab.
    pub tab: Tab,
}

impl State {
    /// Forget the choice and the remembered tracks (a new source).
    pub fn reset(&mut self) {
        self.choice = None;
        self.remembered.clear();
    }

    fn selected(tracks: &[Track]) -> Option<&Track> {
        tracks.iter().find(|track| track.selected)
    }

    /// The columns for these tracks.
    pub fn view(&self, tracks: &[Track]) -> View {
        let mut languages = vec![Choice::Off];
        for lang in [Lang::En, Lang::Id] {
            if tracks.iter().any(|track| track_lang(track) == Some(lang)) {
                languages.push(Choice::Lang(lang));
            }
        }
        let selected = Self::selected(tracks);
        let active = self
            .choice
            .or_else(|| selected.and_then(track_lang).map(Choice::Lang))
            .unwrap_or(Choice::Off);
        let mut rows = vec![];
        if let Choice::Lang(lang) = active {
            let of_language: Vec<(usize, &Track)> = tracks
                .iter()
                .enumerate()
                .filter(|(_, track)| track_lang(track) == Some(lang))
                .collect();
            let labels = number_repeats(
                of_language
                    .iter()
                    .map(|(index, track)| source_label(*index, track.codec.as_deref()))
                    .collect(),
            );
            for ((_, track), label) in of_language.into_iter().zip(labels) {
                rows.push(TrackRow {
                    id: track.id,
                    label,
                    description: Some(track.label.clone())
                        .filter(|text| !text.is_empty() && text != lang.name()),
                    selected: track.selected,
                });
            }
        }
        View {
            languages,
            active,
            tracks: rows,
            selected: selected.map(|track| track.id),
        }
    }

    /// Pick a language. `Some(command)` is the selection to send: `Some(None)` turns subtitles
    /// off, `Some(Some(id))` selects a track; `None` leaves the selection alone.
    pub fn choose(&mut self, language: Choice, tracks: &[Track]) -> Option<Option<i64>> {
        let selected = Self::selected(tracks);
        let selected_language = selected.and_then(track_lang);
        if let (Some(track), Some(lang)) = (selected, selected_language) {
            self.remembered.insert(lang, track.id);
        }
        self.choice = Some(language);
        let Choice::Lang(lang) = language else {
            return Some(None);
        };
        if Some(lang) == selected_language {
            return None;
        }
        let candidates: Vec<&Track> = tracks
            .iter()
            .filter(|track| track_lang(track) == Some(lang))
            .collect();
        let remembered = self.remembered.get(&lang).copied();
        candidates
            .iter()
            .find(|track| Some(track.id) == remembered)
            .or(candidates.first())
            .map(|track| Some(track.id))
    }

    /// Pick a track of the active language.
    pub fn choose_track(&mut self, id: i64, tracks: &[Track]) {
        let language = self.view(tracks).active;
        if let Choice::Lang(lang) = language {
            self.remembered.insert(lang, id);
        }
    }
}

#[cfg(test)]
#[path = "subtitles_tests.rs"]
mod tests;
