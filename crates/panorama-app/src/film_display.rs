use panorama_core::addons::{StreamGroup, StreamSource, StreamState};

/// Film's adapted primary action without service or resume states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primary {
    /// Open account sign-in.
    SignIn,
    /// Await sources; the control is disabled.
    Loading,
    /// No source can play; the control asks the addons again.
    Unavailable,
    /// Start playback using the selected source.
    Play,
}
impl Primary {
    /// Resolve every adapted TSX primary-label row.
    pub fn at(signed_in: bool, loading: bool, playable: bool) -> Self {
        if !signed_in {
            Self::SignIn
        } else if !playable && loading {
            Self::Loading
        } else if !playable {
            Self::Unavailable
        } else {
            Self::Play
        }
    }
    /// Screen-reader and visible action copy.
    pub fn label(self) -> &'static str {
        match self {
            Self::SignIn => "Sign in to watch",
            Self::Loading => "Loading sources",
            Self::Unavailable => "Retry sources",
            Self::Play => "Play",
        }
    }
    /// Whether activating this state can perform an action.
    pub fn enabled(self) -> bool {
        matches!(self, Self::SignIn | Self::Unavailable | Self::Play)
    }
}

/// Optional video and audio badges extracted from a playable source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quality {
    /// Resolution label, if recognized.
    pub video: Option<&'static str>,
    /// Whether the source advertises six-channel audio.
    pub surround: bool,
}
impl Quality {
    /// Parse bounded addon display names, titles and behavior-hint values.
    pub fn parse(text: &str) -> Self {
        let text = text.to_uppercase();
        Self {
            video: if text.contains("2160P") || text.contains("4K") {
                Some("4K")
            } else if text.contains("1080P")
                || text.contains("720P")
                || text
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|s| s == "HD")
            {
                Some("HD")
            } else {
                None
            },
            surround: text.contains("5.1") || text.contains("6CH"),
        }
    }
    /// Inspect names, titles, filenames and additional display hints without logging them.
    pub fn from_source(source: &StreamSource) -> Self {
        let hints = &source.stream.behavior_hints;
        let text = [
            source.name.as_deref(),
            source.title.as_deref(),
            source.description.as_deref(),
            hints.filename.as_deref(),
            hints.binge_group.as_deref(),
        ]
        .into_iter()
        .flatten()
        .chain(hints.other.values().filter_map(|value| value.as_str()))
        .map(|text| text.chars().take(1000).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");
        Self::parse(&text)
    }
}

/// First stream of the first ready group whose target is a URL or torrent info hash.
pub fn first_playable(groups: &[StreamGroup]) -> Option<StreamSource> {
    groups
        .iter()
        .filter_map(|group| match &group.state {
            StreamState::Ready(sources) => Some(sources),
            _ => None,
        })
        .flatten()
        .find(|source| {
            serde_json::to_value(&source.stream).is_ok_and(|value| {
                value.get("url").is_some_and(|url| url.is_string())
                    || value.get("infoHash").is_some()
            })
        })
        .cloned()
}

/// A positive score is visible when votes are positive or absent from addon metadata.
pub fn rating(score: Option<&str>, votes: Option<u64>) -> Option<String> {
    let score = score?.parse::<f64>().ok()?;
    (score.is_finite() && score > 0.0 && votes != Some(0)).then(|| format!("{score:.1}"))
}

/// Addon runtimes ("173 min", "2h53min", "PT2H53M") as "2h 53m"; "45 min" becomes "45m".
/// Shapes it cannot read pass through untouched.
pub fn runtime(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let numbers: Vec<u32> = lower
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse().ok())
        .collect();
    let minutes = match (lower.contains('h'), numbers.as_slice()) {
        (true, [hours]) => hours * 60,
        (true, [hours, minutes]) => hours * 60 + minutes,
        (false, [minutes]) => *minutes,
        _ => return text.to_string(),
    };
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes}m"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h {minutes}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtimes_read_as_hours_and_minutes() {
        for (text, shown) in [
            ("173 min", "2h 53m"),
            ("2h53min", "2h 53m"),
            ("2h 53m", "2h 53m"),
            ("PT2H53M", "2h 53m"),
            ("1 hr 30 min", "1h 30m"),
            ("45 min", "45m"),
            ("120 min", "2h"),
            ("2h", "2h"),
            ("2:53", "2:53"),
            ("", ""),
        ] {
            assert_eq!(runtime(text), shown, "{text}");
        }
    }
    #[test]
    fn every_primary_label_row() {
        for loading in [false, true] {
            for playable in [false, true] {
                assert_eq!(Primary::at(false, loading, playable), Primary::SignIn);
            }
        }
        for (loading, playable, state, label, enabled) in [
            (true, false, Primary::Loading, "Loading sources", false),
            (false, false, Primary::Unavailable, "Retry sources", true),
            (false, true, Primary::Play, "Play", true),
            (true, true, Primary::Play, "Play", true),
        ] {
            let actual = Primary::at(true, loading, playable);
            assert_eq!(actual, state);
            assert_eq!(actual.label(), label);
            assert_eq!(actual.enabled(), enabled);
        }
        assert_eq!(Primary::SignIn.label(), "Sign in to watch");
        assert!(Primary::SignIn.enabled());
    }
    #[test]
    fn quality_names() {
        for text in ["2160p", "4K", "1080p 2160p"] {
            assert_eq!(Quality::parse(text).video, Some("4K"));
        }
        for text in ["1080p", "720p", "HD"] {
            assert_eq!(Quality::parse(text).video, Some("HD"));
        }
        for text in ["5.1", "DD5.1", "DDP5.1", "6CH", "ddp5.1"] {
            assert!(Quality::parse(text).surround);
        }
        assert_eq!(Quality::parse("480p stereo"), Quality::default());
    }
    #[test]
    fn rating_visibility() {
        for score in [
            None,
            Some("0"),
            Some("-1"),
            Some("NaN"),
            Some("inf"),
            Some("unknown"),
        ] {
            assert!(rating(score, None).is_none());
        }
        assert_eq!(rating(Some("7.6"), None), Some("7.6".into()));
        assert!(rating(Some("7.6"), Some(0)).is_none());
        assert_eq!(rating(Some("7.6"), Some(2485)), Some("7.6".into()));
    }
}

pub fn format_time_left(offset: f64, duration: Option<f64>) -> Option<String> {
    let duration = duration.filter(|value| value.is_finite() && *value > 0.0)?;
    if !offset.is_finite() {
        return None;
    }
    let minutes = ((duration - offset).max(0.0) / 60.0).round().max(1.0) as u64;
    let hours = minutes / 60;
    Some(if hours > 0 {
        format!("{hours}h {}m", minutes % 60)
    } else {
        format!("{minutes}m")
    })
}

#[cfg(test)]
mod time_left_tests {
    #[test]
    fn time_left_rounding_and_bounds() {
        for (offset, duration, expected) in [
            (0.0, None, None),
            (0.0, Some(0.0), None),
            (0.0, Some(-1.0), None),
            (0.0, Some(89.0), Some("1m")),
            (0.0, Some(90.0), Some("2m")),
            (0.0, Some(3600.0), Some("1h 0m")),
            (1200.0, Some(6100.0), Some("1h 22m")),
            (100.0, Some(20.0), Some("1m")),
        ] {
            assert_eq!(
                super::format_time_left(offset, duration).as_deref(),
                expected
            );
        }
        assert!(super::format_time_left(f64::NAN, Some(60.0)).is_none());
        assert!(super::format_time_left(0.0, Some(f64::INFINITY)).is_none());
    }
}
