use super::*;

fn roster(friends: &[(&str, MemberState)]) -> Vec<RosterRow> {
    let mut view = preview(true);
    view.roster.truncate(1);
    for (index, (name, state)) in friends.iter().enumerate() {
        let mut row = view.roster[0].clone();
        row.member.member_id = index as u32 + 1;
        row.member.name = (*name).into();
        row.member.state = *state;
        row.member.is_host = false;
        view.roster.push(row);
    }
    view.roster
}

#[test]
fn a_friend_who_stepped_away_does_not_hold_start() {
    let away = Readiness::of(&roster(&[
        ("Sam", MemberState::Ready),
        ("Kim", MemberState::Away),
    ]));
    assert!(away.can_start());
    assert_eq!(
        decide(true, &away, Command::StartWatching),
        Some(Exit::Start)
    );
}

#[test]
fn start_is_disabled_until_every_joined_friend_is_ready() {
    let loading = Readiness::of(&roster(&[
        ("Sam", MemberState::Paused),
        ("Rina", MemberState::Loading),
    ]));
    assert!(!loading.can_start());
    assert_eq!(decide(true, &loading, Command::StartWatching), None);

    let ready = Readiness::of(&roster(&[
        ("Sam", MemberState::Paused),
        ("Rina", MemberState::Ready),
    ]));
    assert!(ready.can_start());
    assert_eq!(
        decide(true, &ready, Command::StartWatching),
        Some(Exit::Start)
    );

    let alone = Readiness::of(&roster(&[]));
    assert!(!alone.can_start(), "nobody to start for yet");
    assert_eq!(decide(true, &alone, Command::StartWatching), None);
}

#[test]
fn start_anyway_is_offered_while_someone_loads() {
    let readiness = Readiness::of(&roster(&[
        ("Sam", MemberState::Ready),
        ("Rina", MemberState::Loading),
        ("Alex", MemberState::Buffering),
    ]));
    assert!(readiness.offers_start_anyway());
    assert_eq!(readiness.waiting, ["Rina", "Alex"]);
    assert_eq!(
        decide(true, &readiness, Command::StartAnyway),
        Some(Exit::Start)
    );
    assert_eq!(
        decide(false, &readiness, Command::StartAnyway),
        None,
        "only the host starts the party"
    );

    let ready = Readiness::of(&roster(&[("Sam", MemberState::Ready)]));
    assert!(!ready.offers_start_anyway());
    assert_eq!(decide(true, &ready, Command::StartAnyway), None);
}

#[test]
fn closing_the_lobby_never_ends_or_leaves_the_party() {
    let joined = Readiness::of(&roster(&[("Sam", MemberState::Loading)]));
    assert_eq!(decide(true, &joined, Command::Escape), Some(Exit::Browse));
    assert_eq!(decide(false, &joined, Command::Escape), None);
    let alone = Readiness::of(&roster(&[]));
    assert_eq!(decide(true, &alone, Command::Escape), Some(Exit::Browse));
}

#[test]
fn the_host_picks_another_film_or_ends_the_party() {
    let joined = Readiness::of(&roster(&[("Sam", MemberState::Ready)]));
    assert_eq!(
        decide(true, &joined, Command::PickAnotherFilm),
        Some(Exit::Browse)
    );
    assert_eq!(decide(false, &joined, Command::PickAnotherFilm), None);
    assert_eq!(
        decide(true, &joined, Command::LeaveParty),
        Some(Exit::Leave)
    );
}

#[test]
fn a_member_lobby_closes_when_the_host_starts() {
    let readiness = Readiness::default();
    assert_eq!(
        decide(false, &readiness, Command::HostStarted),
        Some(Exit::Started)
    );
    assert_eq!(decide(true, &readiness, Command::HostStarted), None);
}

#[test]
fn the_party_plays_the_hosts_selected_release() {
    let film = crate::fixtures::films().remove(0);
    let selection = PlaybackSelection {
        video_id: film.id.clone(),
        film,
        groups: crate::fixtures::player_groups(),
        source: SourceRef {
            group: 0,
            stream: 1,
        },
        start: crate::playback::selection::StartMode::Restart,
        meta_base: None,
        subtitles: Vec::new(),
        late_subtitles: None,
    };
    let media = media_of(&selection).expect("a ready source");
    assert_eq!(media.film_id, "tmdb:100");
    let StreamState::Ready(streams) = &selection.groups[0].state else {
        panic!("fixture streams");
    };
    assert_eq!(
        media.release,
        PartyRelease::of(&selection.groups[0], &streams[1])
    );
    let empty = PlaybackSelection {
        source: SourceRef {
            group: 1,
            stream: 0,
        },
        ..selection
    };
    assert!(
        media_of(&empty).is_none(),
        "an addon with no sources plays nothing"
    );
}

#[test]
fn a_friend_who_cannot_play_is_named_apart_from_the_loading() {
    let readiness = Readiness::of(&roster(&[
        ("Sam", MemberState::Failed),
        ("Rina", MemberState::Ready),
    ]));
    assert_eq!(readiness.waiting, ["Sam"]);
    assert_eq!(readiness.failed, ["Sam"]);
    assert!(readiness.offers_start_anyway());
}

#[test]
fn a_friends_lobby_says_why_the_party_ended() {
    use crate::party::{EndReason, Link};
    use panorama_core::party::ByeReason;
    let mut view = preview(false);
    let said = |view: &PartyView| view::member_status(Some(view), true).to_string();
    view.link = Link::Ended(EndReason::Bye(ByeReason::VersionMismatch));
    assert!(
        said(&view).contains("different versions"),
        "{}",
        said(&view)
    );
    view.link = Link::Ended(EndReason::Bye(ByeReason::HostLeft));
    assert_eq!(said(&view), "The host ended the party.");
    view.link = Link::Ended(EndReason::Lost);
    assert_eq!(said(&view), "Lost the connection to the host.");
}

#[test]
fn a_friends_lobby_waits_for_the_hosts_film_to_load() {
    let mut view = preview(false);
    let set_host = |view: &mut PartyView, state| {
        view.roster
            .iter_mut()
            .find(|row| row.member.is_host)
            .unwrap()
            .member
            .state = state;
    };
    assert!(!host_started(&view), "still in the lobby");
    view.gate = Some(Gate::Ready);
    set_host(&mut view, MemberState::Loading);
    assert!(!host_started(&view), "the host's film is still opening");
    assert_eq!(
        view::member_status(Some(&view), true).to_string(),
        "Billy is loading the film"
    );
    set_host(&mut view, MemberState::Paused);
    assert!(host_started(&view));
    set_host(&mut view, MemberState::Failed);
    assert!(
        host_started(&view),
        "a host who can't play does not hold friends here"
    );
}

#[test]
fn a_friend_joining_mid_start_waits_for_the_roster() {
    // Joining while the host loads: the state can arrive before anyone is listed.
    let mut view = preview(false);
    view.gate = Some(Gate::Ready);
    view.roster.clear();
    assert!(!host_started(&view));
}

#[test]
fn a_long_word_shrinks_the_waiting_room_title_instead_of_breaking() {
    assert_eq!(view::room_title_size("Aftersun", 900.0), 56.0);
    let size = view::room_title_size("Dr. Strangelove or: How I Learned", 300.0);
    assert!(size < 56.0 && 11.0 * size * 0.8 <= 300.5, "{size}");
    assert_eq!(view::room_title_size("Supercalifragilistic", 100.0), 24.0);
}
