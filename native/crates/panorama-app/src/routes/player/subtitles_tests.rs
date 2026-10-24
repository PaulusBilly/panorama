use super::*;

fn track(id: i64, lang: Option<&str>, label: &str, codec: &str, selected: bool) -> Track {
    Track {
        id,
        label: label.into(),
        description: Some(codec.into()),
        lang: lang.map(str::to_owned),
        codec: Some(codec.into()),
        selected,
        external: false,
    }
}

fn tracks() -> Vec<Track> {
    vec![
        track(1, Some("eng"), "English", "subrip", false),
        track(2, Some("eng"), "English SDH", "subrip", false),
        track(3, Some("ind"), "Indonesian", "subrip", false),
        track(4, Some("fra"), "French", "subrip", false),
        track(5, Some("en"), "English", "hdmv_pgs_subtitle", false),
    ]
}

fn select(tracks: &mut [Track], id: Option<i64>) {
    for track in tracks {
        track.selected = Some(track.id) == id;
    }
}

#[test]
fn language_ids_follow_the_runtime_port() {
    assert_eq!(language_id(Some("eng"), None), Some(Lang::En));
    assert_eq!(language_id(Some("EN-us"), None), Some(Lang::En));
    assert_eq!(language_id(Some("pt_BR"), None), None);
    assert_eq!(language_id(Some("ind"), None), Some(Lang::Id));
    assert_eq!(language_id(None, Some("Bahasa Indonesia")), Some(Lang::Id));
    assert_eq!(language_id(None, Some("Indonesian SDH")), Some(Lang::Id));
    assert_eq!(language_id(None, Some("English (CC)")), Some(Lang::En));
    assert_eq!(language_id(None, Some("Englishman")), None);
    assert_eq!(language_id(Some("fra"), Some("French")), None);
    assert_eq!(language_id(None, None), None);
}

#[test]
fn only_offered_languages_present_among_the_tracks_are_listed() {
    let state = State::default();
    let view = state.view(&tracks());
    assert_eq!(
        view.languages,
        [Choice::Off, Choice::Lang(Lang::En), Choice::Lang(Lang::Id)]
    );
    assert_eq!(view.active, Choice::Off);
    assert!(view.tracks.is_empty());
    let french_only = vec![track(4, Some("fra"), "French", "subrip", false)];
    assert_eq!(state.view(&french_only).languages, [Choice::Off]);
    assert_eq!(state.view(&[]).languages, [Choice::Off]);
}

#[test]
fn track_rows_number_repeats_and_describe_only_what_differs() {
    let mut list = tracks();
    select(&mut list, Some(2));
    let view = State::default().view(&list);
    assert_eq!(view.active, Choice::Lang(Lang::En));
    let rows: Vec<_> = view
        .tracks
        .iter()
        .map(|row| {
            (
                row.id,
                row.label.as_str(),
                row.description.as_deref(),
                row.selected,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (1, "Embedded 1 \u{b7} subrip", None, false),
            (2, "Embedded 2 \u{b7} subrip", Some("English SDH"), true),
            (5, "Embedded 5 \u{b7} hdmv_pgs_subtitle", None, false),
        ]
    );
    // Repeated source labels (two tracks with the same codec index text) are numbered.
    assert_eq!(source_label(0, None), "Embedded 1");
}

#[test]
fn repeated_labels_get_a_number() {
    let labels = ["Addon", "Other", "Addon", "Addon"]
        .map(String::from)
        .to_vec();
    assert_eq!(
        number_repeats(labels),
        [
            "Addon \u{b7} 1",
            "Other",
            "Addon \u{b7} 2",
            "Addon \u{b7} 3"
        ]
    );
    assert_eq!(number_repeats(vec![]), Vec::<String>::new());
    assert_eq!(source_label(0, None), "Embedded 1");
}

#[test]
fn off_sends_none_and_a_language_picks_its_first_track() {
    let mut list = tracks();
    let mut state = State::default();
    assert_eq!(state.choose(Choice::Lang(Lang::Id), &list), Some(Some(3)));
    select(&mut list, Some(3));
    assert_eq!(state.view(&list).active, Choice::Lang(Lang::Id));
    assert_eq!(state.choose(Choice::Off, &list), Some(None));
    select(&mut list, None);
    let view = state.view(&list);
    assert_eq!(view.active, Choice::Off);
    assert!(view.tracks.is_empty());
}

#[test]
fn choosing_the_language_already_playing_changes_nothing() {
    let mut list = tracks();
    select(&mut list, Some(1));
    let mut state = State::default();
    assert_eq!(state.choose(Choice::Lang(Lang::En), &list), None);
    assert_eq!(state.view(&list).active, Choice::Lang(Lang::En));
}

#[test]
fn the_last_track_of_each_language_is_remembered_for_the_session() {
    let mut list = tracks();
    let mut state = State::default();
    select(&mut list, Some(1));
    // The user picks the second English track, then moves to Indonesian and back.
    state.choose_track(2, &list);
    select(&mut list, Some(2));
    assert_eq!(state.choose(Choice::Lang(Lang::Id), &list), Some(Some(3)));
    select(&mut list, Some(3));
    assert_eq!(
        state.choose(Choice::Lang(Lang::En), &list),
        Some(Some(2)),
        "English returns to the remembered SDH track"
    );
    select(&mut list, Some(2));
    // Off, then English again: the remembered track, not the first.
    assert_eq!(state.choose(Choice::Off, &list), Some(None));
    select(&mut list, None);
    assert_eq!(state.choose(Choice::Lang(Lang::En), &list), Some(Some(2)));
    state.reset();
    assert_eq!(state.choose(Choice::Lang(Lang::En), &list), Some(Some(1)));
}

#[test]
fn a_language_without_tracks_selects_nothing() {
    let list = vec![track(4, Some("fra"), "French", "subrip", false)];
    let mut state = State::default();
    assert_eq!(state.choose(Choice::Lang(Lang::Id), &list), None);
    assert_eq!(state.view(&list).active, Choice::Lang(Lang::Id));
    assert!(state.view(&list).tracks.is_empty());
}

#[test]
fn addon_files_are_named_after_their_addon_beside_built_in_tracks() {
    let addon = |id, lang| Track {
        external: true,
        ..track(id, Some(lang), "OpenSubtitles", "subrip", false)
    };
    let mut tracks = vec![
        track(1, Some("eng"), "English", "subrip", true),
        addon(8, "eng"),
        addon(9, "eng"),
        addon(10, "ind"),
    ];
    let view = State::default().view(&tracks);
    assert_eq!(
        view.tracks
            .iter()
            .map(|row| (row.label.as_str(), row.description.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("Embedded 1 \u{b7} subrip", None),
            ("OpenSubtitles \u{b7} 1", None),
            ("OpenSubtitles \u{b7} 2", None),
        ]
    );
    // An addon's language is offered even when the video has no track in it.
    select(&mut tracks, None);
    assert_eq!(
        State::default().view(&tracks).languages,
        [Choice::Off, Choice::Lang(Lang::En), Choice::Lang(Lang::Id)]
    );
}

#[test]
fn the_remembered_language_turns_on_once_until_the_viewer_chooses() {
    let mut tracks = vec![track(1, Some("fra"), "French", "subrip", false)];
    let mut state = State::default();
    // Not there yet: an addon's files arrive after the built-in tracks.
    assert_eq!(state.restore(Lang::En, &tracks), None);
    tracks.push(track(8, Some("eng"), "OpenSubtitles", "subrip", false));
    assert_eq!(state.restore(Lang::En, &tracks), Some(8));
    select(&mut tracks, Some(8));
    // Later track lists leave the selection and any choice alone.
    assert_eq!(state.restore(Lang::En, &tracks), None);
    let mut chose_off = State::default();
    chose_off.choose(Choice::Off, &tracks);
    select(&mut tracks, None);
    assert_eq!(chose_off.restore(Lang::En, &tracks), None);
}
