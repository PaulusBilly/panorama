//! Festival and award marks for Awards & Festivals tiles without a MUBI logo, matched to the
//! award's name: the freely licensed Commons set (`assets/awards/CREDITS.md`). A tile with
//! none shows a generic award icon. Also the words for how a film fared.

/// Words in an award's name, and the mark they select.
const COMMONS: [(&str, &str); 17] = [
    ("cannes", "cannes"),
    ("berlin", "berlin"),
    ("venice", "venice"),
    ("toronto", "tiff"),
    ("rotterdam", "iffr"),
    ("new york film festival", "nyff"),
    ("tribeca", "tribeca"),
    ("sxsw", "sxsw"),
    ("european film award", "efa"),
    ("indiewire", "indiewire"),
    ("annecy", "annecy"),
    ("sydney film", "sff"),
    ("idfa", "idfa"),
    ("sundance", "sundance"),
    ("golden globe", "globes"),
    ("critics choice", "critics"),
    ("goya", "goya"),
];

/// How a film fared, as one word for a tile's result column or a card's note.
pub fn result_label(result: panorama_core::credits::AwardResult) -> &'static str {
    use panorama_core::credits::AwardResult;
    match result {
        AwardResult::Won => "Winner",
        AwardResult::RunnerUp => "Runner-up",
        AwardResult::Mention => "Mention",
        AwardResult::Nominated => "Nominee",
        AwardResult::Selected => "Selection",
    }
}

/// The asset path of the mark for an award body, if one is bundled.
pub fn mark(body: &str) -> Option<String> {
    let text = fold(body);
    COMMONS
        .iter()
        .find(|(words, _)| contains_words(&text, words))
        .map(|(_, slug)| format!("awards/{slug}.png"))
}

/// The bytes behind a path [`mark`] returned, for [`crate::assets::Assets`].
pub fn asset(path: &str) -> Option<&'static [u8]> {
    ASSETS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

/// Lowercase with the common Latin accents folded, so "César" and "Cesar" meet.
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            '’' => '\'',
            c => c,
        })
        .collect()
}

/// `words` appears in `text` on word boundaries, so "tiff" does not match "plaintiff".
fn contains_words(text: &str, words: &str) -> bool {
    text.match_indices(words).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + words.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// The Commons marks, as served by [`crate::assets::Assets`].
const ASSETS: [(&str, &[u8]); 17] = [
    (
        "awards/annecy.png",
        include_bytes!("../assets/awards/annecy.png"),
    ),
    (
        "awards/berlin.png",
        include_bytes!("../assets/awards/berlin.png"),
    ),
    (
        "awards/cannes.png",
        include_bytes!("../assets/awards/cannes.png"),
    ),
    (
        "awards/critics.png",
        include_bytes!("../assets/awards/critics.png"),
    ),
    ("awards/efa.png", include_bytes!("../assets/awards/efa.png")),
    (
        "awards/globes.png",
        include_bytes!("../assets/awards/globes.png"),
    ),
    (
        "awards/goya.png",
        include_bytes!("../assets/awards/goya.png"),
    ),
    (
        "awards/idfa.png",
        include_bytes!("../assets/awards/idfa.png"),
    ),
    (
        "awards/iffr.png",
        include_bytes!("../assets/awards/iffr.png"),
    ),
    (
        "awards/indiewire.png",
        include_bytes!("../assets/awards/indiewire.png"),
    ),
    (
        "awards/nyff.png",
        include_bytes!("../assets/awards/nyff.png"),
    ),
    ("awards/sff.png", include_bytes!("../assets/awards/sff.png")),
    (
        "awards/sundance.png",
        include_bytes!("../assets/awards/sundance.png"),
    ),
    (
        "awards/sxsw.png",
        include_bytes!("../assets/awards/sxsw.png"),
    ),
    (
        "awards/tiff.png",
        include_bytes!("../assets/awards/tiff.png"),
    ),
    (
        "awards/tribeca.png",
        include_bytes!("../assets/awards/tribeca.png"),
    ),
    (
        "awards/venice.png",
        include_bytes!("../assets/awards/venice.png"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_find_their_mark() {
        assert_eq!(
            mark("Cannes Film Festival").as_deref(),
            Some("awards/cannes.png")
        );
        assert_eq!(
            mark("Golden Globe Award").as_deref(),
            Some("awards/globes.png")
        );
        assert_eq!(mark("Goya Awards").as_deref(), Some("awards/goya.png"));
        // Whole words only: "tiff" is not in "plaintiff".
        assert_eq!(mark("Plaintiff Award"), None);
        for (_, slug) in COMMONS {
            assert!(asset(&format!("awards/{slug}.png")).is_some(), "{slug}");
        }
    }
}
