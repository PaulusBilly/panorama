use super::TorrentError;
use std::fmt;

/// Validated stream-addon torrent source. Debug never exposes tracker URLs.
#[derive(Clone)]
pub struct TorrentSource {
    pub(super) hash: String,
    pub(super) file_idx: Option<usize>,
    pub(super) dht: bool,
    magnet: String,
    trackers: usize,
}

impl TorrentSource {
    /// Validate BTv1 hex/base32, filter addon sources, and encode magnet trackers.
    pub fn from_stream(
        info_hash: &str,
        file_idx: Option<u32>,
        sources: &[String],
    ) -> Result<Self, TorrentError> {
        let hash = normalize(info_hash).ok_or(TorrentError::InvalidSource)?;
        let dht = !sources.iter().any(|source| source.starts_with("tracker:"))
            || sources.iter().any(|source| {
                source
                    .strip_prefix("dht:")
                    .and_then(normalize)
                    .is_some_and(|value| value == hash)
            });
        let mut magnet = format!("magnet:?xt=urn:btih:{hash}");
        let mut trackers = 0;
        for source in sources {
            let Some(tracker) = source.strip_prefix("tracker:") else {
                continue;
            };
            if tracker.chars().take(2049).count() > 2048
                || tracker.chars().any(|ch| ch.is_control())
            {
                continue;
            }
            let Ok(url) = url::Url::parse(tracker) else {
                continue;
            };
            if !tracker.contains("://")
                || !matches!(url.scheme(), "udp" | "http" | "https")
                || url.host_str().is_none()
            {
                continue;
            }
            magnet.push_str("&tr=");
            magnet.extend(url::form_urlencoded::byte_serialize(tracker.as_bytes()));
            trackers += 1;
            if trackers == 20 {
                break;
            }
        }
        Ok(Self {
            hash,
            dht,
            file_idx: file_idx.map(|idx| idx as usize),
            magnet,
            trackers,
        })
    }

    /// Normalized lowercase hex info hash.
    pub fn info_hash(&self) -> &str {
        &self.hash
    }

    /// Magnet URI containing credentials; do not log this value.
    pub fn magnet_uri(&self) -> &str {
        &self.magnet
    }
}

impl fmt::Debug for TorrentSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TorrentSource")
            .field("info_hash", &self.hash)
            .field("file_idx", &self.file_idx)
            .field("tracker_count", &self.trackers)
            .finish()
    }
}

fn normalize(hash: &str) -> Option<String> {
    if hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(hash.to_ascii_lowercase());
    }
    if hash.len() != 32 {
        return None;
    }
    let mut decoded = Vec::with_capacity(20);
    let (mut bits, mut value) = (0, 0u32);
    for byte in hash.bytes() {
        let digit = match byte.to_ascii_uppercase() {
            b'A'..=b'Z' => byte.to_ascii_uppercase() - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return None,
        };
        value = (value << 5) | u32::from(digit);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            decoded.push((value >> bits) as u8);
            value &= (1 << bits) - 1;
        }
    }
    Some(decoded.iter().map(|byte| format!("{byte:02x}")).collect())
}
