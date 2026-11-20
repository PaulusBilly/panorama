use std::fmt;

use serde::{Deserialize, Serialize};

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
