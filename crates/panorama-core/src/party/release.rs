use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    addons::{StreamGroup, StreamSource, StreamState},
    torrent::TorrentSource,
};

/// A stream's identity across installs that share one account: debrid links change on every
/// lookup while their labels hold, and a torrent is the same file from any addon.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartyRelease {
    /// The manifest id. An `AddonKey`'s suffix is salted per install, so it never matches.
    pub addon: String,
    pub torrent: Option<TorrentRef>,
    /// `name\ntitle\ndescription`, as the addon printed them.
    pub label: String,
    pub binge_group: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TorrentRef {
    /// 40 lowercase hex characters.
    pub info_hash: String,
    /// `None` lets the engine pick the file.
    pub file_idx: Option<u16>,
}

impl fmt::Debug for PartyRelease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PartyRelease")
            .field("torrent", &self.torrent.is_some())
            .finish_non_exhaustive()
    }
}

impl PartyRelease {
    pub fn of(group: &StreamGroup, source: &StreamSource) -> Self {
        Self {
            addon: manifest_id(group).to_owned(),
            torrent: torrent_of(source),
            label: label(source),
            binge_group: source.stream.behavior_hints.binge_group.clone(),
        }
    }
}

/// The member's own copy of `release` as `(group, stream, exact)`. A torrent matches from any
/// addon, then the same addon's stream with the same label; the same addon's binge group is
/// the inexact last resort.
pub fn find(release: &PartyRelease, groups: &[StreamGroup]) -> Option<(usize, usize, bool)> {
    let in_addon = |(_, _, group, _): &Candidate| manifest_id(group) == release.addon;
    let exact = release
        .torrent
        .as_ref()
        .and_then(|torrent| {
            ready(groups).find(|(.., source)| torrent_of(source).as_ref() == Some(torrent))
        })
        .or_else(|| {
            ready(groups)
                .filter(in_addon)
                .find(|(.., source)| label(source) == release.label)
        });
    if let Some((group, stream, ..)) = exact {
        return Some((group, stream, true));
    }
    let binge = release.binge_group.as_deref()?;
    ready(groups)
        .filter(in_addon)
        .find(|(.., source)| source.stream.behavior_hints.binge_group.as_deref() == Some(binge))
        .map(|(group, stream, ..)| (group, stream, false))
}

type Candidate<'a> = (usize, usize, &'a StreamGroup, &'a StreamSource);

fn ready(groups: &[StreamGroup]) -> impl Iterator<Item = Candidate<'_>> {
    groups.iter().enumerate().flat_map(|(at, group)| {
        let streams = match &group.state {
            StreamState::Ready(streams) => streams.as_slice(),
            _ => &[],
        };
        streams
            .iter()
            .enumerate()
            .map(move |(index, source)| (at, index, group, source))
    })
}

fn manifest_id(group: &StreamGroup) -> &str {
    let key = group.addon.as_str();
    key.split_once('#').map_or(key, |(id, _)| id)
}

/// Kept equal to the app's `LastSource` label so a host and a member build the same one.
fn label(source: &StreamSource) -> String {
    [&source.name, &source.title, &source.description]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

fn torrent_of(source: &StreamSource) -> Option<TorrentRef> {
    let value = serde_json::to_value(&source.stream).ok()?;
    let file_idx = match value.get("fileIdx") {
        None | Some(serde_json::Value::Null) => None,
        Some(index) => Some(u16::try_from(index.as_u64()?).ok()?),
    };
    let hash = value.get("infoHash")?.as_str()?;
    let source = TorrentSource::from_stream(hash, None, &[]).ok()?;
    Some(TorrentRef {
        info_hash: source.info_hash().to_owned(),
        file_idx,
    })
}
