use super::model::PlaybackError;
use panorama_core::{addons::StreamSource, media::fetch::MediaSource, torrent::TorrentSource};

#[derive(Debug)]
pub enum ResolvedSource {
    Http(MediaSource),
    Torrent(TorrentSource),
}

pub fn resolve(stream: &StreamSource) -> Result<ResolvedSource, PlaybackError> {
    let value =
        serde_json::to_value(&stream.stream).map_err(|_| PlaybackError::SourceUnavailable)?;
    if let Some(url) = value.get("url").and_then(|value| value.as_str()) {
        return MediaSource::new(url)
            .map(ResolvedSource::Http)
            .map_err(|_| PlaybackError::SourceUnavailable);
    }
    if let Some(hash) = value.get("infoHash").and_then(|value| value.as_str()) {
        let index = value
            .get("fileIdx")
            .and_then(|value| value.as_u64())
            .map(u32::try_from)
            .transpose()
            .map_err(|_| PlaybackError::SourceUnavailable)?;
        let sources = value
            .get("sources")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        return TorrentSource::from_stream(hash, index, &sources)
            .map(ResolvedSource::Torrent)
            .map_err(|_| PlaybackError::SourceUnavailable);
    }
    Err(PlaybackError::SourceUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_source_branches_are_redacted() {
        for (value, kind) in [
            (
                serde_json::json!({"url":"https://media.invalid/video.mp4?token=private-token"}),
                1,
            ),
            (
                serde_json::json!({"url":"http://media.invalid/video.mp4?token=private-token"}),
                1,
            ),
            (
                serde_json::json!({"url":"ftp://media.invalid/video.mp4?token=private-token"}),
                0,
            ),
            (
                serde_json::json!({"infoHash":"dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"}),
                2,
            ),
            (
                serde_json::json!({"infoHash":"dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c","fileIdx":2,"sources":["tracker:https://media.invalid/video.mp4?token=private-token"]}),
                2,
            ),
            (serde_json::json!({"ytId":"video"}), 0),
        ] {
            let source = StreamSource {
                stream: serde_json::from_value(value).unwrap(),
                name: None,
                title: None,
                description: None,
            };
            let result = resolve(&source);
            assert_eq!(
                match &result {
                    Ok(ResolvedSource::Http(_)) => 1,
                    Ok(ResolvedSource::Torrent(_)) => 2,
                    Err(_) => 0,
                },
                kind
            );
            let debug = format!("{result:?}");
            assert!(!debug.contains("media.invalid"));
            assert!(!debug.contains("private-token"));
        }
    }
}
