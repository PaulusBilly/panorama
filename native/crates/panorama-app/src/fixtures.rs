use panorama_core::addons::FilmDetails;

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
