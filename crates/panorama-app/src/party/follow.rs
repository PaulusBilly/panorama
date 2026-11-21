//! Picking the local tracks that match the host's choice. The host names a track by language,
//! title, codec and where it comes from, never by mpv id, so each machine finds its own copy.
use super::PartySync;
use crate::{
    playback::model::Track,
    routes::player::subtitles::{Source, language_id},
};
use panorama_core::party::{TrackChoice, TrackKey};

/// What to select locally. `None` leaves that kind of track alone.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Follow {
    pub audio: Option<i64>,
    /// `Some(None)` turns subtitles off.
    pub subtitle: Option<Option<i64>>,
}

/// How the host's player names `track` to the party.
pub fn key_of(track: &Track) -> TrackKey {
    let title = match Source::of(track) {
        Source::Addon(name) => name,
        Source::Embedded => track.label.clone(),
    };
    TrackKey {
        language: track.lang.clone(),
        title: Some(title),
        codec: track.codec.clone(),
        external: track.external,
    }
}

/// The host's choice from its selected tracks. `None` until its player has listed any.
pub fn choice_of(audio: &[Track], subtitles: &[Track]) -> Option<TrackChoice> {
    if audio.is_empty() && subtitles.is_empty() {
        return None;
    }
    let selected = |tracks: &[Track]| tracks.iter().find(|track| track.selected).map(key_of);
    Some(TrackChoice {
        audio: selected(audio),
        subtitle: selected(subtitles),
    })
}

/// The tracks to select so this player matches `choice`, for the kinds `sync` follows.
/// A track already on, or a host track nothing here matches, asks for nothing.
pub fn follow(
    sync: PartySync,
    choice: &TrackChoice,
    audio: &[Track],
    subtitles: &[Track],
) -> Follow {
    let audio = match (&choice.audio, sync.audio) {
        (Some(key), true) => find(audio, key).filter(|id| !is_on(audio, *id)),
        _ => None,
    };
    let subtitle = match (&choice.subtitle, sync.subtitles) {
        (_, false) => None,
        (None, true) => subtitles.iter().any(|track| track.selected).then_some(None),
        (Some(key), true) => find(subtitles, key)
            .filter(|id| !is_on(subtitles, *id))
            .map(Some),
    };
    Follow { audio, subtitle }
}

/// Whether `choice` has an answer here yet for each kind `sync` follows: a match, or the host
/// having turned it off. Without one, a later track list (an addon's subtitles arrive late)
/// may still bring a match.
pub fn settled(
    sync: PartySync,
    choice: &TrackChoice,
    audio: &[Track],
    subtitles: &[Track],
) -> bool {
    let audio_ok = !sync.audio
        || choice
            .audio
            .as_ref()
            .is_none_or(|key| find(audio, key).is_some());
    let subtitles_ok = !sync.subtitles
        || choice
            .subtitle
            .as_ref()
            .is_none_or(|key| find(subtitles, key).is_some());
    audio_ok && subtitles_ok
}

fn is_on(tracks: &[Track], id: i64) -> bool {
    tracks.iter().any(|track| track.id == id && track.selected)
}

fn find(tracks: &[Track], key: &TrackKey) -> Option<i64> {
    tracks
        .iter()
        .filter_map(|track| fit(track, key).map(|score| (score, track.id)))
        .fold(None, |best: Option<(u8, i64)>, found| match best {
            Some(best) if best.0 >= found.0 => Some(best),
            _ => Some(found),
        })
        .map(|(_, id)| id)
}

/// How well `track` matches `key`, or `None` when it is a different language. Where the track
/// comes from outranks its title, which outranks its codec.
fn fit(track: &Track, key: &TrackKey) -> Option<u8> {
    let mine = key_of(track);
    let same_language = match (&mine.language, &key.language) {
        (Some(mine), Some(theirs)) => same_language(mine, theirs),
        (None, None) => mine.title == key.title,
        _ => false,
    };
    same_language.then(|| {
        4 * u8::from(mine.external == key.external)
            + 2 * u8::from(mine.title == key.title)
            + u8::from(mine.codec == key.codec)
    })
}

fn same_language(a: &str, b: &str) -> bool {
    match (language_id(Some(a), None), language_id(Some(b), None)) {
        (Some(a), Some(b)) => a == b,
        _ => a.eq_ignore_ascii_case(b),
    }
}
