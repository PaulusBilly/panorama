//! Language and track choices of the Subtitles panel (`PlayerSubtitlesPopover.tsx`).
use crate::playback::model::Track;
use panorama_core::addons::SubtitleSource;
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
    /// The stored code.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Id => "id",
        }
    }
    pub fn parse(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Self::En),
            "id" => Some(Self::Id),
            _ => None,
        }
    }
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

/// The panel's tabs.
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
    /// Position in [`Tab::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

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

/// Where a subtitle track comes from, remembered across films.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    /// Built into the video file.
    Embedded,
    /// An addon's file, by the addon's name.
    Addon(String),
}

impl Source {
    /// An addon's file is named after the addon (its mpv title).
    pub fn of(track: &Track) -> Self {
        if track.external {
            Self::Addon(track.label.clone())
        } else {
            Self::Embedded
        }
    }

    /// The stored form.
    pub fn key(&self) -> String {
        match self {
            Self::Embedded => "embedded".into(),
            Self::Addon(name) => format!("addon:{name}"),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        match key {
            "embedded" => Some(Self::Embedded),
            _ => key
                .strip_prefix("addon:")
                .filter(|name| !name.is_empty())
                .map(|name| Self::Addon(name.into())),
        }
    }
}

/// The addon files worth adding, the auto-pick first: only languages the panel offers (each
/// file is a download mpv makes before any later command), the remembered language first and,
/// within it, the remembered addon.
pub fn order_files(
    files: Vec<SubtitleSource>,
    lang: Option<Lang>,
    source: Option<&Source>,
) -> Vec<SubtitleSource> {
    let mut files: Vec<_> = files
        .into_iter()
        .filter(|file| language_id(Some(&file.lang), None).is_some())
        .collect();
    files.sort_by_key(|file| {
        let same_lang = lang.is_some() && language_id(Some(&file.lang), None) == lang;
        let same_source = matches!(source, Some(Source::Addon(name)) if *name == file.addon);
        (!same_lang, !same_source)
    });
    files
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
    /// The choice was restored, not made in the panel, so a track from the remembered
    /// source may still replace it.
    restored: bool,
    /// The track restore last asked mpv for: track lists sent before mpv applies it still
    /// show it off, and asking again would only queue the same command.
    requested: Option<i64>,
    remembered: HashMap<Lang, i64>,
    /// The narrow layout's tab.
    pub tab: Tab,
}

impl State {
    /// Turn on `lang` for tracks that just arrived, unless the viewer chose for this source or
    /// a track was already on. The first track plays until one from the remembered `source`
    /// arrives. Returns the track to select.
    pub fn restore(
        &mut self,
        lang: Lang,
        source: Option<&Source>,
        tracks: &[Track],
    ) -> Option<i64> {
        let started = self.choice.is_some() || Self::selected(tracks).is_some();
        if started && !self.restored {
            return None;
        }
        let mut candidates = tracks
            .iter()
            .filter(|track| track_lang(track) == Some(lang));
        let preferred = candidates
            .clone()
            .find(|track| source.is_some_and(|source| Source::of(track) == *source));
        let target = if self.restored {
            preferred?
        } else {
            preferred.or_else(|| candidates.next())?
        };
        self.choice = Some(Choice::Lang(lang));
        self.restored = true;
        if target.selected || self.requested == Some(target.id) {
            return None;
        }
        self.requested = Some(target.id);
        Some(target.id)
    }

    /// Forget the choice and the remembered tracks (a new source).
    pub fn reset(&mut self) {
        self.choice = None;
        self.restored = false;
        self.requested = None;
        self.remembered.clear();
    }

    /// Whether subtitles are on: the language chosen or restored, before mpv confirms the
    /// track, else whatever mpv has selected.
    pub fn showing(&self, tracks: &[Track]) -> bool {
        match self.choice {
            Some(Choice::Lang(_)) => true,
            Some(Choice::Off) => false,
            None => Self::selected(tracks).is_some(),
        }
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
            // An addon's file is named after the addon (its mpv title); a built-in track by
            // its position in the video.
            let labels = number_repeats(
                of_language
                    .iter()
                    .map(|(index, track)| {
                        if track.external {
                            track.label.clone()
                        } else {
                            source_label(*index, track.codec.as_deref())
                        }
                    })
                    .collect(),
            );
            for ((_, track), label) in of_language.into_iter().zip(labels) {
                rows.push(TrackRow {
                    id: track.id,
                    label,
                    description: Some(track.label.clone())
                        .filter(|text| !track.external && !text.is_empty() && text != lang.name()),
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
        self.restored = false;
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
        self.restored = false;
        let language = self.view(tracks).active;
        if let Choice::Lang(lang) = language {
            self.remembered.insert(lang, id);
        }
    }
}

#[cfg(test)]
#[path = "subtitles_tests.rs"]
mod tests;
