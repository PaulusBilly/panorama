use super::*;

fn ep(id: &str, season: u32, number: u32, released: Option<&str>) -> Episode {
    Episode {
        id: id.into(),
        season,
        number,
        numbered: true,
        title: id.into(),
        overview: None,
        thumbnail: None,
        released: released.map(Into::into),
    }
}

fn next<'a>(episodes: &'a [Episode], from: &str) -> Option<&'a str> {
    next_after(episodes, from, "2024-06-01").map(|episode| episode.id.as_str())
}

#[test]
fn moves_within_a_season_and_across_a_season_end() {
    let episodes = [
        ep("s1e2", 1, 2, Some("2024-01-08")),
        ep("s1e1", 1, 1, Some("2024-01-01")),
        ep("s2e1", 2, 1, Some("2024-02-01")),
    ];
    assert_eq!(next(&episodes, "s1e1"), Some("s1e2"));
    assert_eq!(next(&episodes, "s1e2"), Some("s2e1"));
}

#[test]
fn skips_unaired_and_stops_at_the_end() {
    let episodes = [
        ep("s1e1", 1, 1, Some("2024-01-01")),
        ep("s1e2", 1, 2, Some("2024-06-02")),
        ep("s1e3", 1, 3, Some("2024-06-09")),
    ];
    assert_eq!(next(&episodes, "s1e1"), None);
    assert_eq!(
        next_after(&episodes, "s1e1", "2024-06-02").map(|e| e.id.as_str()),
        Some("s1e2")
    );
    assert_eq!(next_after(&episodes, "s1e3", "2025-01-01"), None);
    assert_eq!(next(&episodes, "missing"), None);
}

#[test]
fn undated_episodes_count_as_aired() {
    let episodes = [ep("a", 1, 1, None), ep("b", 1, 2, None)];
    assert_eq!(next(&episodes, "a"), Some("b"));
}

#[test]
fn specials_are_neither_entered_nor_left() {
    let episodes = [
        ep("sp1", 0, 1, Some("2023-01-01")),
        ep("s1e1", 1, 1, Some("2024-01-01")),
        ep("sp2", 0, 2, Some("2023-02-01")),
        ep("s2e1", 2, 1, Some("2024-02-01")),
    ];
    assert_eq!(next(&episodes, "s1e1"), Some("s2e1"));
    assert_eq!(next(&episodes, "s2e1"), None);
    assert_eq!(next(&episodes, "sp1"), Some("sp2"));
    assert_eq!(next(&episodes, "sp2"), None);
}
