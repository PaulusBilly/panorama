use panorama_core::{
    addons::{AddonKey, FilmDetails, Kind, StreamGroup, StreamSource, StreamState, StreamsEvent},
    stremio::{ContinueItem, FilmMeta, ResumePoint},
};

/// Bundled stills served for `fixture://<name>` image URLs; one is deliberately bright.
pub const STILLS: [(&str, &[u8]); 3] = [
    ("dusk.jpg", include_bytes!("../assets/fixtures/dusk.jpg")),
    ("night.jpg", include_bytes!("../assets/fixtures/night.jpg")),
    (
        "bright.jpg",
        include_bytes!("../assets/fixtures/bright.jpg"),
    ),
];

/// Which still each preview film uses. The catalog pick (tmdb:100) gets the bright one so the
/// signed-out hero exercises the scrims on it. The Continue films (101, 105, 106) differ in
/// title and still, and the bright one lands in the row.
const FILM_STILLS: [usize; 9] = [2, 0, 1, 0, 1, 2, 1, 2, 0];

pub fn films() -> Vec<FilmDetails> {
    (0..9)
        .map(|index| FilmDetails {
            id: format!("tmdb:{}", 100 + index),
            kind: Kind::Movie,
            name: ["Aftersun", "Perfect Days", "Past Lives"][index % 3].into(),
            release_info: Some((2022 + index % 3).to_string()),
            released: None,
            director: vec![["Charlotte Wells", "Wim Wenders", "Celine Song"][index % 3].into()],
            origin_country: (index % 3 == 1).then(|| "Japan".into()),
            original_name: (index % 3 == 1).then(|| "パーフェクト・デイズ".into()),
            runtime: Some("102 min".into()),
            genres: vec!["Drama".into()],
            description: Some("A quiet, observant portrait shaped by memory and time.".into()),
            cast: vec![],
            people: vec![],
            poster: None,
            background: url::Url::parse(&format!("fixture://{}", STILLS[FILM_STILLS[index]].0))
                .ok(),
            logo: None,
            imdb_rating: Some(["7.6", "7.9", "7.8"][index % 3].into()),
            links: vec![],
            episodes: vec![],
        })
        .collect()
}

/// Match by name containment or complete director equality.
pub fn search(query: &str) -> Vec<FilmDetails> {
    let query = query.to_lowercase();
    films()
        .into_iter()
        .filter(|film| {
            film.name.to_lowercase().contains(&query)
                || film
                    .director
                    .iter()
                    .any(|director| director.to_lowercase() == query)
        })
        .collect()
}

/// The film-details fixture, with the selected card's opaque identifier.
pub fn details(id: &str) -> Option<FilmDetails> {
    let mut film = films().into_iter().find(|film| film.id == id)?;
    // The first film stands in for a TMDB-backed metadata addon; the rest for Cinemeta.
    if film.id == "tmdb:100" {
        film.people = people();
    }
    Some(film)
}

pub fn continue_items() -> Vec<ContinueItem> {
    [("tmdb:101", 0.2), ("tmdb:105", 0.55), ("tmdb:106", 0.8)]
        .into_iter()
        .filter_map(|(id, fraction)| {
            let film = films().into_iter().find(|film| film.id == id)?;
            let duration_secs = 6600.0;
            Some(ContinueItem {
                film: FilmMeta {
                    id: film.id,
                    name: film.name,
                    poster: film.poster,
                },
                resume: ResumePoint {
                    offset_secs: duration_secs * fraction,
                    duration_secs,
                },
            })
        })
        .collect()
}

/// One fixture addon with a single HD, 5.1 source, or none when `empty`.
pub fn streams(empty: bool) -> StreamsEvent {
    if empty {
        return StreamsEvent::NoSources;
    }
    let stream = serde_json::from_value(serde_json::json!({
        "url": "https://fixture.invalid/aftersun.mp4",
        "name": "Fixture 1080p",
        "description": "1080p DDP5.1",
    }));
    let key = serde_json::from_value::<AddonKey>("fixture#0".into());
    match (stream, key) {
        (Ok(stream), Ok(addon)) => StreamsEvent::Groups(vec![StreamGroup {
            addon,
            name: "Fixture".into(),
            state: StreamState::Ready(vec![StreamSource {
                stream,
                name: Some("Fixture 1080p".into()),
                title: Some("1080p DDP5.1".into()),
                description: Some("1080p DDP5.1".into()),
            }]),
        }]),
        _ => StreamsEvent::NoSources,
    }
}

#[cfg(test)]
mod search_tests {
    #[test]
    fn fixture_stream_is_playable_hd_surround() {
        let panorama_core::addons::StreamsEvent::Groups(groups) = super::streams(false) else {
            panic!("fixture groups");
        };
        let source = crate::film_display::first_playable(&groups).expect("playable");
        let quality = crate::film_display::Quality::from_source(&source);
        assert_eq!((quality.video, quality.surround), (Some("HD"), true));
        assert!(matches!(
            super::streams(true),
            panorama_core::addons::StreamsEvent::NoSources
        ));
    }

    #[test]
    fn fixture_matching_rule() {
        assert_eq!(super::search("past").len(), 3);
        assert_eq!(super::search("PAST").len(), 3);
        assert_eq!(super::search("Charlotte Wells").len(), 3);
        assert!(super::search("Charlotte").is_empty());
        assert!(super::search("night").is_empty());
        assert_eq!(super::search("").len(), 9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continue_films_are_told_apart_on_screen() {
        let names = continue_items()
            .into_iter()
            .map(|item| item.film.name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["Perfect Days", "Past Lives", "Aftersun"]);
    }
    #[test]
    fn preview_films() {
        let films = films();
        assert_eq!(films.len(), 9);
        for (index, film) in films.iter().enumerate() {
            assert_eq!(film.id, format!("tmdb:{}", 100 + index));
            assert_eq!(
                film.name,
                ["Aftersun", "Perfect Days", "Past Lives"][index % 3]
            );
            assert_eq!(film.release_info, Some((2022 + index % 3).to_string()));
            assert_eq!(
                film.director,
                vec![["Charlotte Wells", "Wim Wenders", "Celine Song"][index % 3].to_string()]
            );
            assert_eq!(
                film.origin_country.as_deref(),
                (index % 3 == 1).then_some("Japan")
            );
            assert!(film.poster.is_none() && film.logo.is_none());
            assert_eq!(
                film.description.as_deref(),
                Some("A quiet, observant portrait shaped by memory and time.")
            );
            assert_eq!(film.runtime.as_deref(), Some("102 min"));
            assert_eq!(
                film.imdb_rating.as_deref(),
                Some(["7.6", "7.9", "7.8"][index % 3])
            );
            assert!(film.background.is_some());
        }
    }
}

fn fixture_source(value: serde_json::Value, name: &str, title: &str) -> Option<StreamSource> {
    Some(StreamSource {
        stream: serde_json::from_value(value).ok()?,
        name: Some(name.into()),
        title: Some(title.into()),
        description: Some(title.into()),
    })
}

/// Three addons for the player's Sources panel: one with sources, one empty, one failed.
pub fn player_groups() -> Vec<StreamGroup> {
    let group = |key: &str, name: &str, state: StreamState| {
        serde_json::from_value::<AddonKey>(key.into())
            .ok()
            .map(|addon| StreamGroup {
                addon,
                name: name.into(),
                state,
            })
    };
    let sources = [
        fixture_source(
            serde_json::json!({"url": "https://fixture.invalid/aftersun-2160p.mp4"}),
            "Fixture 2160p HDR",
            "Aftersun 2160p HDR DDP5.1 14.2 GB",
        ),
        fixture_source(
            serde_json::json!({"url": "https://fixture.invalid/aftersun-1080p.mp4"}),
            "Fixture 1080p",
            "Aftersun 1080p BluRay 6.8 GB",
        ),
        fixture_source(
            serde_json::json!({"url": "https://fixture.invalid/aftersun-720p.mp4"}),
            "Fixture 720p",
            "Aftersun 720p WEB-DL 2.1 GB with a long release name that wraps onto a second line",
        ),
    ]
    .into_iter()
    .flatten()
    .chain(fixture_source(
        serde_json::json!({"infoHash": "0123456789abcdef0123456789abcdef01234567"}),
        "Fixture Torrent 1080p",
        "Aftersun 1080p BluRay x265
👤 128 💾 4.2 GB",
    ))
    .collect();
    [
        group("fixture#0", "Fixture Streams", StreamState::Ready(sources)),
        group("quiet#1", "Quiet Addon", StreamState::Empty),
        group(
            "broken#2",
            "Broken Addon",
            StreamState::Failed(panorama_core::addons::FailureKind::Offline),
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Cast and crew for the film page: a few with stills for photos, the rest with none.
pub fn people() -> Vec<panorama_core::credits::Person> {
    [
        ("Charlotte Wells", "Director, Screenplay", Some(0)),
        ("Paul Mescal", "Cast", Some(1)),
        ("Frankie Corio", "Cast", Some(2)),
        ("Celia Rowlson-Hall", "Cast", None),
        ("Sally Messham", "Cast", Some(0)),
        ("Brooklyn Toulson", "Cast", None),
        ("Spike Fearn", "Cast", Some(1)),
        ("Harry Perdios", "Cast", None),
        ("Gregory Oke", "Cinematography", None),
        ("Blair McClendon", "Editor", Some(2)),
        ("Oliver Coates", "Music", None),
    ]
    .into_iter()
    .map(|(name, role, still)| panorama_core::credits::Person {
        name: name.into(),
        role: role.into(),
        photo: still.and_then(|index: usize| {
            url::Url::parse(&format!("fixture://{}", STILLS[index].0)).ok()
        }),
    })
    .collect()
}

/// A sample of MUBI's awards and festivals for Aftersun. Logos come from the network, so
/// fixture tiles fall back to the bundled marks.
pub fn awards() -> Vec<panorama_core::credits::Award> {
    use panorama_core::credits::{AwardResult::*, Event};
    [
        (
            "BAFTA Awards",
            "baftas",
            9,
            Some("Outstanding Debut"),
            2023,
            Won,
        ),
        (
            "BAFTA Awards",
            "baftas",
            9,
            Some("Best Leading Actor"),
            2023,
            Nominated,
        ),
        (
            "BAFTA Awards",
            "baftas",
            9,
            Some("Outstanding British Film"),
            2023,
            Nominated,
        ),
        (
            "Independent Spirit Awards",
            "isas",
            7,
            Some("Best First Feature"),
            2023,
            Won,
        ),
        (
            "Cannes Film Festival",
            "cannes",
            1,
            Some("French Touch Prize of the Critics' Week Jury"),
            2022,
            Won,
        ),
        (
            "Cannes Film Festival",
            "cannes",
            1,
            Some("Critics Week Grand Prize"),
            2022,
            Nominated,
        ),
        (
            "Sundance Film Festival",
            "sundance",
            2,
            None,
            2023,
            Selected,
        ),
        (
            "Toronto International Film Festival",
            "toronto",
            8,
            None,
            2022,
            Selected,
        ),
        (
            "Academy Awards",
            "oscars",
            4,
            Some("Best Actor"),
            2023,
            Nominated,
        ),
        (
            "European Film Awards",
            "efas",
            30,
            Some("European Actor"),
            2022,
            Nominated,
        ),
        ("New York Film Festival", "nyff", 25, None, 2022, Selected),
    ]
    .into_iter()
    .map(
        |(body, slug, id, category, year, result)| panorama_core::credits::Award {
            body: body.into(),
            category: category.map(str::to_owned),
            year: Some(year),
            result,
            event: Some(Event {
                id,
                slug: slug.into(),
                logo: None,
                cover: None,
            }),
        },
    )
    .collect()
}
