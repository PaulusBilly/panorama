use panorama_core::addons::{
    AddonKey, FilmDetails, StreamGroup, StreamSource, StreamState, StreamsEvent,
};

/// The nine film records from runtime/fake-runtime.ts, in exactly the same order.
pub fn films() -> Vec<FilmDetails> {
    (0..9)
        .map(|index| FilmDetails {
            id: format!("tmdb:{}", 100 + index),
            name: ["Aftersun", "Perfect Days", "Past Lives"][index % 3].into(),
            release_info: Some((2022 + index % 3).to_string()),
            director: vec![["Charlotte Wells", "Wim Wenders", "Celine Song"][index % 3].into()],
            origin_country: (index % 3 == 1).then(|| "Japan".into()),
            runtime: None,
            genres: vec![],
            description: None,
            cast: vec![],
            poster: None,
            background: None,
            logo: None,
            imdb_rating: None,
            links: vec![],
        })
        .collect()
}

/// Match Electron's fixture name containment or complete director equality.
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

/// Electron's film-details fixture, with the selected card's opaque identifier.
pub fn details(id: &str) -> Option<FilmDetails> {
    let mut film = films().into_iter().find(|film| film.id == id)?;
    film.runtime = Some("102 min".into());
    film.genres = vec!["Drama".into()];
    film.description = Some("A quiet, observant portrait shaped by memory and time.".into());
    film.imdb_rating = Some(
        ["7.6", "7.9", "7.8"][id
            .strip_prefix("tmdb:")?
            .parse::<usize>()
            .ok()?
            .checked_sub(100)?
            % 3]
        .into(),
    );
    Some(film)
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
    fn matches_electron_preview_source() {
        let source = include_str!("../../../../runtime/fake-runtime.ts");
        for literal in [
            "length: 9",
            "100 + index",
            "2022 + (index % 3)",
            "[\"Aftersun\", \"Perfect Days\", \"Past Lives\"]",
            "[\"Charlotte Wells\", \"Wim Wenders\", \"Celine Song\"]",
            "[null, \"Japan\", null]",
            "posterUrl: null",
            "landscapeUrl: null",
            "logoUrl: null",
            "description: null",
        ] {
            assert!(source.contains(literal), "{literal}");
        }
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
            assert!(
                film.poster.is_none()
                    && film.background.is_none()
                    && film.logo.is_none()
                    && film.description.is_none()
            );
        }
    }
}
