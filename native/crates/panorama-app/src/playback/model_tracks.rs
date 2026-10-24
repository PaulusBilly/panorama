use super::Track;

pub(super) fn track_value(track: &panorama_mpv::Track, index: usize) -> Track {
    let label = track
        .title
        .as_ref()
        .filter(|title| {
            !title.trim().is_empty()
                && !title.trim().starts_with("http:")
                && !title.trim().starts_with("https:")
        })
        .cloned()
        .or_else(|| {
            track
                .lang
                .as_deref()
                .filter(|code| !code.trim().is_empty())
                .map(|code| language(code).unwrap_or(code).to_owned())
        })
        .unwrap_or_else(|| format!("Track {}", index + 1));
    Track {
        id: track.id,
        label,
        description: track.codec.clone(),
        lang: track.lang.clone(),
        codec: track.codec.clone(),
        selected: track.selected,
        external: track.external,
    }
}

fn language(code: &str) -> Option<&'static str> {
    match code {
        "en" | "eng" => Some("English"),
        "fr" | "fra" | "fre" => Some("French"),
        "de" | "deu" | "ger" => Some("German"),
        "es" | "spa" => Some("Spanish"),
        "it" | "ita" => Some("Italian"),
        "ja" | "jpn" => Some("Japanese"),
        "ko" | "kor" => Some("Korean"),
        "zh" | "zho" | "chi" => Some("Chinese"),
        "pt" | "por" => Some("Portuguese"),
        "ru" | "rus" => Some("Russian"),
        "th" | "tha" => Some("Thai"),
        "ar" | "ara" => Some("Arabic"),
        "hi" | "hin" => Some("Hindi"),
        "nl" | "nld" | "dut" => Some("Dutch"),
        _ => None,
    }
}
