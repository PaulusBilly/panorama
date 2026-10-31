use panorama_core::addons::{FilmDetails, StreamGroup, StreamSource, StreamState};
use url::Url;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceRef {
    pub group: usize,
    pub stream: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum StartMode {
    #[default]
    Restart,
    Resume(f64),
}

impl StartMode {
    pub fn seconds(self) -> f64 {
        match self {
            Self::Resume(seconds) if seconds.is_finite() => seconds.max(0.0),
            _ => 0.0,
        }
    }
}

#[derive(Clone)]
pub struct PlaybackSelection {
    pub film: FilmDetails,
    /// What is playing: the film's own id, or an episode's.
    pub video_id: String,
    pub groups: Vec<StreamGroup>,
    pub source: SourceRef,
    pub start: StartMode,
    pub meta_base: Option<Url>,
    /// The addons' subtitle files for the film, when the lookup finished before Play.
    pub subtitles: Vec<panorama_core::addons::SubtitleSource>,
    /// The lookup still in flight when Play started; its files join the play as they arrive.
    pub late_subtitles: Option<SubtitleLookup>,
}

/// The addons' subtitle lookup for a film, shared by the film page and the player.
pub type SubtitleLookup = futures::future::Shared<
    futures::future::BoxFuture<'static, Vec<panorama_core::addons::SubtitleSource>>,
>;

impl PlaybackSelection {
    pub fn selected(&self) -> Option<&StreamSource> {
        match &self.groups.get(self.source.group)?.state {
            StreamState::Ready(streams) => streams.get(self.source.stream),
            _ => None,
        }
    }
}

impl std::fmt::Debug for PlaybackSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaybackSelection")
            .field("source", &self.source)
            .field("start", &self.start)
            .finish_non_exhaustive()
    }
}

/// The stream to start with: the best of [`ranked`].
pub fn first_source(groups: &[StreamGroup]) -> Option<SourceRef> {
    ranked(groups).first().copied()
}

/// Every playable stream, most reliable first: direct links, then torrents by seeders, with
/// 1080p ahead of 4K unless the 4K stream is healthy, oversized files pushed down and dead
/// torrents last. Ties keep the account's addon order.
pub fn ranked(groups: &[StreamGroup]) -> Vec<SourceRef> {
    let mut scored = groups
        .iter()
        .enumerate()
        .filter_map(|(group, item)| match &item.state {
            StreamState::Ready(streams) => Some((group, streams)),
            _ => None,
        })
        .flat_map(|(group, streams)| {
            streams
                .iter()
                .enumerate()
                .filter_map(move |(stream, source)| {
                    let resolved = super::source::resolve(source).ok()?;
                    let direct = matches!(resolved, super::source::ResolvedSource::Http(_));
                    Some((score(source, direct), SourceRef { group, stream }))
                })
        })
        .collect::<Vec<_>>();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, source)| source).collect()
}

/// Reliability score from what addons print in a stream's labels (Torrentio-style
/// "2160p … 👤 140 💾 4.1 GB"). Unknown fields score as middling, never as best.
fn score(source: &StreamSource, direct: bool) -> i32 {
    let text = [&source.name, &source.title, &source.description]
        .into_iter()
        .flatten()
        .map(|text| text.to_uppercase())
        .collect::<Vec<_>>()
        .join(" ");
    let seeders = after(&text, "👤").map(|value| value as u32);
    let size = after(&text, "💾").map(|value| {
        if text.contains(" MB") && !text.contains(" GB") {
            value / 1024.0
        } else {
            value
        }
    });
    let transport = match (direct, seeders) {
        (true, _) => 1000,
        (false, Some(0)) => -10_000,
        (false, Some(seeders)) => seeders.min(200) as i32 * 2,
        (false, None) => 60,
    };
    let healthy = direct || seeders.is_some_and(|seeders| seeders >= 100);
    let resolution = if text.contains("2160P") || text.contains("4K") || text.contains("UHD") {
        if healthy && size.is_none_or(|size| size <= 30.0) {
            350
        } else {
            120
        }
    } else if text.contains("1080P") {
        300
    } else if text.contains("720P") {
        200
    } else if text.contains("480P") || text.contains("360P") {
        50
    } else {
        150
    };
    let weight = match size {
        Some(size) if size > 40.0 => -300,
        Some(size) if size > 20.0 => -150,
        _ => 0,
    };
    transport + resolution + weight
}

/// The number printed after `marker`, as in "👤 140" or "💾 4.1 GB".
fn after(text: &str, marker: &str) -> Option<f32> {
    let rest = text.split_once(marker)?.1.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod ranking {
    use super::*;
    use panorama_core::addons::AddonKey;

    fn torrent(hash: char, label: &str) -> StreamSource {
        source(
            serde_json::json!({ "infoHash": hash.to_string().repeat(40), "fileIdx": 0 }),
            label,
        )
    }

    fn link(label: &str) -> StreamSource {
        source(
            serde_json::json!({ "url": "https://cdn.test/film.mkv" }),
            label,
        )
    }

    fn source(stream: serde_json::Value, label: &str) -> StreamSource {
        StreamSource {
            stream: serde_json::from_value(stream).unwrap(),
            name: Some("Addon".into()),
            title: Some(label.into()),
            description: Some(label.into()),
        }
    }

    fn groups(groups: Vec<Vec<StreamSource>>) -> Vec<StreamGroup> {
        groups
            .into_iter()
            .enumerate()
            .map(|(index, streams)| StreamGroup {
                addon: serde_json::from_value::<AddonKey>(format!("addon#{index}").into()).unwrap(),
                name: format!("Addon {index}"),
                state: StreamState::Ready(streams),
            })
            .collect()
    }

    fn at(group: usize, stream: usize) -> SourceRef {
        SourceRef { group, stream }
    }

    #[test]
    fn a_seeded_1080p_torrent_beats_a_4k_remux_listed_first() {
        let groups = groups(vec![vec![
            torrent('a', "Film 2160p REMUX\n👤 12 💾 61.2 GB"),
            torrent('b', "Film 1080p WEB-DL\n👤 140 💾 4.1 GB"),
        ]]);
        assert_eq!(first_source(&groups), Some(at(0, 1)));
    }

    #[test]
    fn a_direct_link_beats_any_torrent() {
        let groups = groups(vec![
            vec![torrent('a', "Film 1080p\n👤 900 💾 3 GB")],
            vec![link("Film 1080p")],
        ]);
        assert_eq!(first_source(&groups), Some(at(1, 0)));
    }

    #[test]
    fn a_healthy_4k_torrent_may_lead() {
        let groups = groups(vec![vec![
            torrent('a', "Film 1080p\n👤 20 💾 3 GB"),
            torrent('b', "Film 2160p WEB-DL\n👤 400 💾 14 GB"),
        ]]);
        assert_eq!(first_source(&groups), Some(at(0, 1)));
    }

    #[test]
    fn torrents_without_seeders_go_last_and_every_source_is_ranked_once() {
        let groups = groups(vec![vec![
            torrent('a', "Film 1080p\n👤 0 💾 3 GB"),
            torrent('b', "Film 720p\n👤 8 💾 1 GB"),
            torrent('c', "Film"),
        ]]);
        assert_eq!(ranked(&groups), [at(0, 1), at(0, 2), at(0, 0)]);
    }
}
