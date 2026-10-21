//! Which film Home features, and which films the Continue row lists.
use crate::continue_watching::ContinueCard;
use panorama_core::addons::FilmDetails;

/// The hero's film: the newest film in progress, or the catalog pick.
#[derive(Clone, Copy, Debug)]
pub enum Featured<'a> {
    Resume(&'a ContinueCard),
    Pick(&'a FilmDetails),
}

impl<'a> Featured<'a> {
    pub fn film(&self) -> &'a FilmDetails {
        match self {
            Self::Resume(card) => &card.film,
            Self::Pick(film) => film,
        }
    }
    /// The primary action's label, in sentence case.
    pub fn action_label(&self) -> &'static str {
        match self {
            Self::Resume(_) => "Continue",
            Self::Pick(_) => "Watch",
        }
    }
}

/// The first viewport's films. The hero never appears in the row.
#[derive(Debug)]
pub struct Lineup<'a> {
    pub hero: Option<Featured<'a>>,
    pub row: &'a [ContinueCard],
}

/// Signed in with items: hero = newest item, row = the rest. Otherwise hero = catalog pick
/// (first catalog film) and no row.
pub fn lineup<'a>(
    signed_in: bool,
    cards: &'a [ContinueCard],
    catalog: &'a [FilmDetails],
) -> Lineup<'a> {
    match (signed_in, cards.split_first()) {
        (true, Some((first, rest))) => Lineup {
            hero: Some(Featured::Resume(first)),
            row: rest,
        },
        _ => Lineup {
            hero: catalog.first().map(Featured::Pick),
            row: &[],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use panorama_core::stremio::ResumePoint;

    fn card(film: FilmDetails) -> ContinueCard {
        ContinueCard {
            film,
            resume: ResumePoint {
                offset_secs: 10.0,
                duration_secs: 100.0,
            },
            progress: 0.1,
        }
    }

    fn ids(lineup: &Lineup<'_>) -> (Option<String>, Vec<String>) {
        (
            lineup.hero.map(|hero| hero.film().id.clone()),
            lineup.row.iter().map(|card| card.film.id.clone()).collect(),
        )
    }

    #[test]
    fn signed_in_features_the_newest_and_lists_the_rest() {
        let catalog = crate::fixtures::films();
        let cards = vec![card(catalog[1].clone()), card(catalog[4].clone())];
        let lineup = lineup(true, &cards, &catalog);
        assert_eq!(
            ids(&lineup),
            (Some("tmdb:101".into()), vec!["tmdb:104".into()])
        );
        assert_eq!(lineup.hero.unwrap().action_label(), "Continue");
    }

    #[test]
    fn signed_out_ignores_continue_cards() {
        let catalog = crate::fixtures::films();
        let cards = vec![card(catalog[1].clone())];
        let lineup = lineup(false, &cards, &catalog);
        assert_eq!(ids(&lineup), (Some("tmdb:100".into()), vec![]));
        assert_eq!(lineup.hero.unwrap().action_label(), "Watch");
    }

    #[test]
    fn one_card_leaves_an_empty_row() {
        let catalog = crate::fixtures::films();
        let cards = vec![card(catalog[4].clone())];
        assert_eq!(
            ids(&lineup(true, &cards, &catalog)),
            (Some("tmdb:104".into()), vec![])
        );
    }

    #[test]
    fn no_cards_features_the_catalog_pick() {
        let catalog = crate::fixtures::films();
        let lineup = lineup(true, &[], &catalog);
        assert!(matches!(lineup.hero, Some(Featured::Pick(_))));
        assert_eq!(ids(&lineup), (Some("tmdb:100".into()), vec![]));
    }

    #[test]
    fn nothing_to_feature_without_cards_or_catalog() {
        assert_eq!(ids(&lineup(true, &[], &[])), (None, vec![]));
        assert_eq!(ids(&lineup(false, &[], &[])), (None, vec![]));
    }
}
