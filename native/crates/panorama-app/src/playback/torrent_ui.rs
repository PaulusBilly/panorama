//! Torrent display state: health for the player, the seeders parser for the Sources panel.
use panorama_core::{addons::StreamSource, torrent::TorrentState};

/// One reading of the engine's stats, reduced to what the player shows. Carries no hash, URL
/// or token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TorrentHealth {
    pub state: TorrentState,
    pub peers_live: u32,
    pub peers_connecting: u32,
    pub download_bps: u64,
    /// Milliseconds since the host started this torrent source.
    pub since_ms: u64,
}

impl TorrentHealth {
    /// The state before the engine has reported anything.
    pub fn connecting() -> Self {
        Self {
            state: TorrentState::Connecting,
            peers_live: 0,
            peers_connecting: 0,
            download_bps: 0,
            since_ms: 0,
        }
    }

    /// "<n> peers · <rate>/s", once any peer is known.
    pub fn caption(&self) -> Option<String> {
        let peers = match self.peers_live {
            0 => return None,
            1 => "1 peer".to_owned(),
            count => format!("{count} peers"),
        };
        Some(format!("{peers} · {}", rate(self.download_bps)))
    }
}

/// Bytes per second as MB/s, with one decimal under 10 (like `formatMbps`).
pub fn rate(bytes_per_second: u64) -> String {
    format!(
        "{} MB/s",
        super::health::format_mbps(bytes_per_second as f64 / 1_000_000.0)
    )
}

/// Whether an addon stream is a torrent (carries an info hash).
pub fn is_torrent(stream: &StreamSource) -> bool {
    serde_json::to_value(&stream.stream)
        .ok()
        .is_some_and(|value| value.get("infoHash").is_some())
}

const SCAN_LIMIT: usize = 1000;
const MAX_DIGITS: usize = 6;

/// The seeders count in an addon title ("👤 N", "Seeders: N", "Seeds: N") and the text with
/// that token (and adjacent whitespace) removed. Untrusted text: only ASCII digits are read, a
/// number over six digits, or one followed by a separator or letter ("1,234", "12.5k"), is
/// ignored, and the earliest marker in the text wins.
pub fn split_seeders(text: &str) -> (String, Option<u32>) {
    let end = text
        .char_indices()
        .nth(SCAN_LIMIT)
        .map_or(text.len(), |(index, _)| index);
    // ASCII lowercasing keeps byte offsets identical to `text`.
    let lower = text[..end].to_ascii_lowercase();
    let found = ["👤", "seeders", "seeds", "seeder"]
        .into_iter()
        .filter_map(|marker| {
            let mut from = 0;
            while let Some(at) = lower[from..].find(marker) {
                let start = from + at;
                if let Some((count, stop)) =
                    digits_after(&lower, start + marker.len(), marker != "👤")
                {
                    return Some((start, stop, count));
                }
                from = start + marker.len();
            }
            None
        })
        .min_by_key(|(start, ..)| *start);
    let Some((start, stop, count)) = found else {
        return (text.to_owned(), None);
    };
    let blank = |c: char| matches!(c, ' ' | '\t' | '\u{a0}');
    let mut before = &text[..start];
    let mut after = text[stop..].trim_start_matches(blank);
    if after.is_empty() || after.starts_with('\n') {
        before = before.trim_end_matches(blank);
        if after.is_empty() {
            before = before.trim_end();
        } else if before.is_empty() || before.ends_with('\n') {
            after = &after[1..];
        }
    }
    (format!("{before}{after}"), Some(count))
}

/// The seeders count alone.
pub fn parse_seeders(text: &str) -> Option<u32> {
    split_seeders(text).1
}

/// Digits at `from` (after optional blanks and, for words, a colon or equals sign), and where
/// they end.
fn digits_after(text: &str, from: usize, word: bool) -> Option<(u32, usize)> {
    let blank = |c: char| matches!(c, ' ' | '\u{a0}');
    let mut rest = text[from..].trim_start_matches(blank);
    if word {
        rest = rest.strip_prefix([':', '=']).unwrap_or(rest);
        rest = rest.trim_start_matches(blank);
    }
    let start = text.len() - rest.len();
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > MAX_DIGITS {
        return None;
    }
    if rest[digits..]
        .chars()
        .next()
        .is_some_and(|next| matches!(next, ',' | '.') || next.is_alphabetic())
    {
        return None;
    }
    Some((rest[..digits].parse().ok()?, start + digits))
}

/// "42 seeders".
pub fn seeders_label(count: u32) -> String {
    match count {
        1 => "1 seeder".into(),
        count => format!("{count} seeders"),
    }
}

#[cfg(test)]
#[path = "torrent_ui_tests.rs"]
mod tests;
