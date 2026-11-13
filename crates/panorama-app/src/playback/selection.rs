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

/// The source a film or episode last played from, so a resume opens the same release. Holds
/// no stream URL, which may carry an account token: a torrent by its hash and file, anything
/// else by the labels the addon printed for it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LastSource {
    addon: String,
    /// `infoHash:fileIdx` for a torrent.
    torrent: Option<String>,
    label: String,
    binge_group: Option<String>,
}

impl LastSource {
    /// What is playing in `selection`, if anything is.
    pub fn of(selection: &PlaybackSelection) -> Option<Self> {
        let group = selection.groups.get(selection.source.group)?;
        Some(Self::from_stream(
            group.addon.as_str(),
            selection.selected()?,
        ))
    }

    fn from_stream(addon: &str, source: &StreamSource) -> Self {
        let value = serde_json::to_value(&source.stream).unwrap_or_default();
        let torrent = value["infoHash"].as_str().map(|hash| {
            format!(
                "{}:{}",
                hash.to_lowercase(),
                value["fileIdx"].as_u64().unwrap_or(0)
            )
        });
        Self {
            addon: addon.to_owned(),
            torrent,
            label: label(source),
            binge_group: source.stream.behavior_hints.binge_group.clone(),
        }
    }

    /// The same stream among `groups`, if its addon still offers it: by torrent, then by label,
    /// as debrid links change on every lookup while their labels hold.
    pub fn find(&self, groups: &[StreamGroup]) -> Option<SourceRef> {
        let ranked = ranked(groups);
        let candidates = || {
            ranked.iter().filter_map(|&at| {
                let group = groups.get(at.group)?;
                let StreamState::Ready(streams) = &group.state else {
                    return None;
                };
                (group.addon.as_str() == self.addon).then(|| (at, streams.get(at.stream)))
            })
        };
        let same = |at: SourceRef, source: Option<&StreamSource>| {
            source.map(|source| (at, Self::from_stream(&self.addon, source)))
        };
        candidates()
            .filter_map(|(at, source)| same(at, source))
            .find(|(_, other)| self.torrent.is_some() && other.torrent == self.torrent)
            .or_else(|| {
                candidates()
                    .filter_map(|(at, source)| same(at, source))
                    .find(|(_, other)| other.label == self.label)
            })
            .map(|(at, _)| at)
    }

    /// The best stream from the same release, when the stream itself is gone.
    pub fn release(&self, groups: &[StreamGroup]) -> Option<SourceRef> {
        binge_source(groups, self.binge_group.as_deref()?)
    }
}

fn label(source: &StreamSource) -> String {
    [&source.name, &source.title, &source.description]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The stream to start with: the best of [`ranked`].
pub fn first_source(groups: &[StreamGroup]) -> Option<SourceRef> {
    ranked(groups).first().copied()
}

/// The best stream in an addon's binge group, so an episode continues the last one's release.
pub fn binge_source(groups: &[StreamGroup], group: &str) -> Option<SourceRef> {
    ranked(groups).into_iter().find(|source| {
        match groups.get(source.group).map(|item| &item.state) {
            Some(StreamState::Ready(streams)) => streams.get(source.stream).is_some_and(|item| {
                item.stream.behavior_hints.binge_group.as_deref() == Some(group)
            }),
            _ => false,
        }
    })
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

    fn binged(stream: serde_json::Value, label: &str, group: &str) -> StreamSource {
        let mut stream = stream;
        stream["behaviorHints"] = serde_json::json!({ "bingeGroup": group });
        source(stream, label)
    }

    fn playing(groups: Vec<StreamGroup>, at: SourceRef) -> LastSource {
        LastSource::from_stream(
            groups[at.group].addon.as_str(),
            match &groups[at.group].state {
                StreamState::Ready(streams) => &streams[at.stream],
                _ => unreachable!(),
            },
        )
    }

    #[test]
    fn the_last_torrent_is_found_again_over_a_better_ranked_one() {
        let before = groups(vec![vec![
            torrent('a', "Film 1080p\n👤 900 💾 3 GB"),
            torrent('b', "Film 720p\n👤 5 💾 1 GB"),
        ]]);
        let last = playing(before, at(0, 1));
        // A later lookup lists it elsewhere, under a refreshed label.
        let after = groups(vec![vec![
            torrent('c', "Film 2160p\n👤 800 💾 9 GB"),
            torrent('a', "Film 1080p\n👤 950 💾 3 GB"),
            torrent('b', "Film 720p\n👤 6 💾 1 GB"),
        ]]);
        assert_eq!(first_source(&after), Some(at(0, 0)));
        assert_eq!(last.find(&after), Some(at(0, 2)));
    }

    #[test]
    fn a_debrid_link_is_found_by_its_label_when_its_url_changes() {
        let link_at = |url: &str, label: &str| source(serde_json::json!({ "url": url }), label);
        let last = playing(
            groups(vec![vec![link_at(
                "https://rd.test/token-1/a.mkv",
                "[RD+] Film 1080p",
            )]]),
            at(0, 0),
        );
        let after = groups(vec![vec![
            link_at("https://rd.test/token-2/b.mkv", "[RD+] Film 2160p"),
            link_at("https://rd.test/token-2/a.mkv", "[RD+] Film 1080p"),
        ]]);
        assert_eq!(last.find(&after), Some(at(0, 1)));
        let stored = serde_json::to_string(&last).unwrap();
        assert!(!stored.contains("token"), "{stored}");
    }

    #[test]
    fn a_gone_stream_falls_back_to_its_release_and_never_to_another_addon() {
        let last = playing(
            groups(vec![vec![binged(
                serde_json::json!({ "infoHash": "a".repeat(40) }),
                "Film 1080p WEB",
                "addon|1080p|WEB",
            )]]),
            at(0, 0),
        );
        let after = groups(vec![
            vec![
                torrent('d', "Film 2160p\n👤 900"),
                binged(
                    serde_json::json!({ "infoHash": "e".repeat(40) }),
                    "Film 1080p WEB v2",
                    "addon|1080p|WEB",
                ),
            ],
            vec![torrent('a', "Film 1080p WEB")],
        ]);
        // The same torrent from a different addon is not the same source.
        assert_eq!(last.find(&after), None);
        assert_eq!(last.release(&after), Some(at(0, 1)));
    }
}
