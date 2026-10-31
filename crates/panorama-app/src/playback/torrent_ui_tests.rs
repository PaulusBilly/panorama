use super::*;

#[test]
fn seeders_are_read_from_the_common_forms() {
    for (text, expected) in [
        ("Movie 1080p\n👤 42 💾 1.4 GB", Some(42)),
        ("👤42", Some(42)),
        ("Seeders: 1200", Some(1200)),
        ("seeders 7", Some(7)),
        ("Seeds: 0", Some(0)),
        ("Name | Seeders:   15 | ThePirateBay", Some(15)),
        ("👤 999999", Some(999_999)),
    ] {
        assert_eq!(parse_seeders(text), expected, "{text}");
    }
}

#[test]
fn seeders_are_omitted_when_absent_or_untrusted() {
    for text in [
        "",
        "Movie 1080p",
        "👤",
        "👤 many",
        "Seeders: -5",
        "👤 1000000",
        "Seeders: 99999999999999999999",
        "42 seeders",
        "👤 1,234",
        "Seeders: 12.5k",
        "👤 12k",
        "Seeds: 3.",
    ] {
        assert_eq!(parse_seeders(text), None, "{text}");
    }
    // Digits past the scan window are never read.
    let far = format!("{} 👤 5", "x".repeat(2000));
    assert_eq!(parse_seeders(&far), None);
}

#[test]
fn a_bad_first_marker_does_not_hide_a_later_one() {
    assert_eq!(parse_seeders("👤 x 👤 8"), Some(8));
}

#[test]
fn the_caption_waits_for_peers_and_formats_the_rate() {
    let mut health = TorrentHealth::connecting();
    assert_eq!(health.caption(), None);
    health.peers_connecting = 3;
    assert_eq!(health.caption(), None);
    health.peers_live = 1;
    health.download_bps = 2_450_000;
    assert_eq!(health.caption().as_deref(), Some("1 peer · 2.5 MB/s"));
    health.peers_live = 12;
    health.download_bps = 14_900_000;
    assert_eq!(health.caption().as_deref(), Some("12 peers · 15 MB/s"));
}

#[test]
fn the_earliest_marker_wins() {
    assert_eq!(parse_seeders("Seeders: 5 | 👤 9"), Some(5));
    assert_eq!(parse_seeders("👤 9 | Seeders: 5"), Some(9));
}

#[test]
fn the_matched_token_and_adjacent_space_are_removed() {
    for (text, clean, count) in [
        (
            "Aftersun 1080p\n👤 128 💾 4.2 GB",
            "Aftersun 1080p\n💾 4.2 GB",
            128,
        ),
        ("Movie\nSeeders: 1", "Movie", 1),
        ("Movie 👤 42\nBluRay", "Movie\nBluRay", 42),
        ("Movie\n👤 42\nBluRay", "Movie\nBluRay", 42),
        ("👤 7 Movie", "Movie", 7),
        ("Movie | Seeders: 15 | TPB", "Movie | | TPB", 15),
    ] {
        assert_eq!(
            split_seeders(text),
            (clean.to_owned(), Some(count)),
            "{text}"
        );
    }
    assert_eq!(
        split_seeders("Movie 👤 many"),
        ("Movie 👤 many".to_owned(), None)
    );
}

#[test]
fn seeders_wording() {
    assert_eq!(seeders_label(1), "1 seeder");
    assert_eq!(seeders_label(0), "0 seeders");
    assert_eq!(seeders_label(42), "42 seeders");
}
