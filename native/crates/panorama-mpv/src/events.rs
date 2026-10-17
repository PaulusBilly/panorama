use crate::{AudioDevice, CacheState, PlayerEvent, Track, TrackKind};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    None,
    String(String),
    Flag(bool),
    Integer(i64),
    Double(f64),
    Array(Vec<Node>),
    Map(Vec<(String, Node)>),
}

impl Node {
    pub(crate) fn get(&self, key: &str) -> &Self {
        if let Self::Map(entries) = self {
            for (name, value) in entries {
                if name == key {
                    return value;
                }
            }
        }
        &Self::None
    }

    pub(crate) fn string(&self) -> Option<String> {
        match self {
            Self::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    fn flag(&self) -> bool {
        matches!(self, Self::Flag(true))
    }

    pub(crate) fn number(&self) -> Option<f64> {
        match self {
            Self::Double(v) if v.is_finite() => Some(*v),
            Self::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub(crate) fn integer(&self) -> Option<i64> {
        match self {
            Self::Integer(v) => Some(*v),
            _ => None,
        }
    }
}

pub(crate) fn tracks(node: &Node) -> Vec<Track> {
    let Node::Array(entries) = node else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id").integer()?;
            let kind = match entry.get("type").string()?.as_str() {
                "audio" => TrackKind::Audio,
                "video" => TrackKind::Video,
                "sub" => TrackKind::Subtitle,
                _ => return None,
            };
            Some(Track {
                id,
                kind,
                title: entry.get("title").string(),
                lang: entry.get("lang").string(),
                codec: entry.get("codec").string(),
                default: entry.get("default").flag(),
                forced: entry.get("forced").flag(),
                external: entry.get("external").flag(),
                hearing_impaired: entry.get("hearing-impaired").flag(),
                selected: entry.get("selected").flag(),
            })
        })
        .collect()
}

pub(crate) struct Properties {
    cache: CacheState,
}

impl Default for Properties {
    fn default() -> Self {
        Self {
            cache: CacheState {
                playback_speed: 1.0,
                ..Default::default()
            },
        }
    }
}

impl Properties {
    pub(crate) fn event(&mut self, name: &str, value: Node) -> Vec<PlayerEvent> {
        let mut events = Vec::new();
        let event = match name {
            "time-pos" => PlayerEvent::TimePos(value.number()),
            "duration" => PlayerEvent::Duration(value.number()),
            "pause" => {
                self.cache.paused = value.flag();
                events.push(PlayerEvent::DemuxerCacheState(self.cache.clone()));
                PlayerEvent::Pause(value.flag())
            }
            "paused-for-cache" => {
                self.cache.buffering = value.flag();
                events.push(PlayerEvent::DemuxerCacheState(self.cache.clone()));
                PlayerEvent::PausedForCache(value.flag())
            }
            "cache-buffering-state" => PlayerEvent::CacheBufferingState(value.number()),
            "hwdec-current" => PlayerEvent::HwdecCurrent(value.string()),
            "track-list" => PlayerEvent::TrackList(tracks(&value)),
            "audio-device-list" => {
                let devices = if let Node::Array(entries) = value {
                    entries
                        .iter()
                        .filter_map(|entry| {
                            Some(AudioDevice {
                                name: entry.get("name").string()?,
                                description: entry.get("description").string()?,
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                PlayerEvent::AudioDeviceList(devices)
            }
            "sub-text" => PlayerEvent::SubText(value.string()),
            "sub-start/full" => PlayerEvent::SubStart(value.number()),
            "sub-end/full" => PlayerEvent::SubEnd(value.number()),
            _ => {
                match name {
                    "demuxer-cache-duration" => self.cache.buffered_seconds = value.number(),
                    "demuxer-cache-time" => self.cache.cache_end_seconds = value.number(),
                    "demuxer-cache-state" => {
                        self.cache.forward_bytes = value.get("fw-bytes").integer()
                    }
                    "cache-speed" => self.cache.input_bytes_per_second = value.number(),
                    "speed" => self.cache.playback_speed = value.number().unwrap_or(1.0),
                    _ => return events,
                }
                PlayerEvent::DemuxerCacheState(self.cache.clone())
            }
        };
        events.push(event);
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_parse_owned_node_tree_and_ignore_malformed_entries() {
        let fields = [
            ("id", Node::Integer(7)),
            ("type", Node::String("sub".into())),
            ("title", Node::String("SDH".into())),
            ("lang", Node::String("en".into())),
            ("codec", Node::String("subrip".into())),
            ("default", Node::Flag(true)),
            ("forced", Node::Flag(false)),
            ("external", Node::Flag(true)),
            ("hearing-impaired", Node::Flag(true)),
            ("selected", Node::Flag(true)),
        ];
        let node = Node::Array(vec![
            Node::Map(fields.into_iter().map(|(k, v)| (k.into(), v)).collect()),
            Node::None,
        ]);
        let parsed = tracks(&node);
        assert_eq!(
            parsed,
            vec![Track {
                id: 7,
                kind: TrackKind::Subtitle,
                title: Some("SDH".into()),
                lang: Some("en".into()),
                codec: Some("subrip".into()),
                default: true,
                forced: false,
                external: true,
                hearing_impaired: true,
                selected: true
            }]
        );
        assert!(tracks(&Node::None).is_empty());
    }

    #[test]
    fn unavailable_cache_resets_measurements() {
        let mut properties = Properties::default();
        properties.event("demuxer-cache-duration", Node::Double(3.0));
        let events = properties.event("demuxer-cache-duration", Node::None);
        assert!(
            matches!(&events[0], PlayerEvent::DemuxerCacheState(s) if s.buffered_seconds.is_none() && s.playback_speed == 1.0)
        );
        properties.event("pause", Node::Flag(true));
        let events = properties.event("cache-speed", Node::Double(1_000_000.0));
        assert!(
            matches!(&events[0], PlayerEvent::DemuxerCacheState(s) if s.paused && s.input_bytes_per_second == Some(1_000_000.0))
        );
    }
}
