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
    pub groups: Vec<StreamGroup>,
    pub source: SourceRef,
    pub start: StartMode,
    pub meta_base: Option<Url>,
}

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

pub fn first_source(groups: &[StreamGroup]) -> Option<SourceRef> {
    groups.iter().enumerate().find_map(|(group, item)| {
        let StreamState::Ready(streams) = &item.state else {
            return None;
        };
        streams
            .iter()
            .position(|stream| super::source::resolve(stream).is_ok())
            .map(|stream| SourceRef { group, stream })
    })
}
