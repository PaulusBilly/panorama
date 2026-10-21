use stremio_core::types::library::LibraryItem;

use super::{CoreSession, FilmMeta, ResumePoint, progress::resume_point};

const CAP: usize = 20;

#[derive(Clone, Debug, PartialEq)]
pub struct ContinueItem {
    pub film: FilmMeta,
    /// Where to resume; already past the resume rule.
    pub resume: ResumePoint,
}

impl ContinueItem {
    pub fn progress(&self) -> f32 {
        (self.resume.offset_secs / self.resume.duration_secs) as f32
    }
}

impl CoreSession {
    /// Films worth resuming, last watched first, at most 20. Empty while signed
    /// out. Reads the rehydrated library, so it works offline.
    pub fn continue_watching(&self) -> Vec<ContinueItem> {
        let Some(running) = self.running().filter(|_| self.is_signed_in()) else {
            return Vec::new();
        };
        let model = running.runtime.model().unwrap_or_else(|p| p.into_inner());
        select(model.ctx.inner.library.items.values())
    }
}

// Mirrors the core's `is_in_continue_watching`: a removed item that is still
// temporary shows up, so films played but never saved appear.
pub(super) fn select<'a>(items: impl IntoIterator<Item = &'a LibraryItem>) -> Vec<ContinueItem> {
    let mut found: Vec<_> = items
        .into_iter()
        .filter(|item| item.r#type == "movie" && (!item.removed || item.temp))
        .filter_map(|item| Some((item, resume_point(&item.state)?)))
        .collect();
    let seen = |item: &LibraryItem| item.state.last_watched.unwrap_or(item.mtime);
    found.sort_by(|(a, _), (b, _)| seen(b).cmp(&seen(a)).then_with(|| a.id.cmp(&b.id)));
    found
        .into_iter()
        .take(CAP)
        .map(|(item, resume)| ContinueItem {
            film: FilmMeta {
                id: item.id.clone(),
                name: item.name.clone(),
                poster: item.poster.clone(),
            },
            resume,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    struct Case<'a> {
        id: &'a str,
        kind: &'a str,
        removed: bool,
        temp: bool,
        offset_secs: u64,
        duration_secs: u64,
        last_watched: Option<&'a str>,
        mtime: &'a str,
    }

    const T1: &str = "2024-01-01T00:00:00Z";
    const T2: &str = "2024-02-01T00:00:00Z";
    const T3: &str = "2024-03-01T00:00:00Z";

    fn case(id: &str) -> Case<'_> {
        Case {
            id,
            kind: "movie",
            removed: false,
            temp: false,
            offset_secs: 600,
            duration_secs: 7200,
            last_watched: Some(T1),
            mtime: T1,
        }
    }

    fn ids(cases: &[Case]) -> Vec<String> {
        let library: Vec<LibraryItem> = cases
            .iter()
            .map(|c| {
                serde_json::from_value(json!({
                    "_id": c.id, "name": c.id, "type": c.kind,
                    "removed": c.removed, "temp": c.temp, "_mtime": c.mtime,
                    "state": {
                        "lastWatched": c.last_watched,
                        "timeWatched": 0,
                        "timeOffset": c.offset_secs * 1000,
                        "overallTimeWatched": 0,
                        "timesWatched": 0,
                        "flaggedWatched": 0,
                        "duration": c.duration_secs * 1000,
                    },
                }))
                .unwrap()
            })
            .collect();
        select(&library).into_iter().map(|c| c.film.id).collect()
    }

    #[test]
    fn selects_only_movies_that_are_visible() {
        let cases = [
            Case {
                kind: "series",
                ..case("series")
            },
            Case {
                kind: "other",
                ..case("other")
            },
            Case {
                removed: true,
                ..case("removed")
            },
            Case {
                removed: true,
                temp: true,
                ..case("removed-temp")
            },
            Case {
                temp: true,
                ..case("temp")
            },
            case("saved"),
        ];
        assert_eq!(ids(&cases), ["removed-temp", "saved", "temp"]);
    }

    #[test]
    fn applies_the_resume_boundaries() {
        let cases = [
            Case {
                offset_secs: 29,
                ..case("offset-29")
            },
            Case {
                offset_secs: 30,
                ..case("offset-30")
            },
            Case {
                offset_secs: 100,
                duration_secs: 159,
                ..case("left-59")
            },
            Case {
                offset_secs: 100,
                duration_secs: 160,
                ..case("left-60")
            },
        ];
        assert_eq!(ids(&cases), ["left-60", "offset-30"]);
    }

    #[test]
    fn orders_by_last_watched_with_mtime_fallback_then_id() {
        let cases = [
            Case {
                mtime: T3,
                ..case("old-watch-new-mtime")
            },
            Case {
                last_watched: None,
                mtime: T2,
                ..case("mtime-only")
            },
            Case {
                last_watched: Some(T3),
                ..case("newest")
            },
            case("b-tie"),
            case("a-tie"),
        ];
        assert_eq!(
            ids(&cases),
            [
                "newest",
                "mtime-only",
                "a-tie",
                "b-tie",
                "old-watch-new-mtime"
            ]
        );
    }

    #[test]
    fn keeps_the_twenty_newest_of_twenty_five() {
        let names: Vec<String> = (0..25).map(|n| format!("f{n:02}")).collect();
        let stamps: Vec<String> = (1..=25)
            .map(|d| format!("2024-03-{d:02}T00:00:00Z"))
            .collect();
        let cases: Vec<Case> = names
            .iter()
            .zip(&stamps)
            .map(|(id, at)| Case {
                last_watched: Some(at),
                ..case(id)
            })
            .collect();
        assert_eq!(
            ids(&cases),
            [
                "f24", "f23", "f22", "f21", "f20", "f19", "f18", "f17", "f16", "f15", "f14", "f13",
                "f12", "f11", "f10", "f09", "f08", "f07", "f06", "f05"
            ]
        );
    }

    #[test]
    fn maps_film_and_resume() {
        let items: Vec<LibraryItem> = vec![
            serde_json::from_value(json!({
                "_id": "m", "name": "Heat", "type": "movie", "removed": false,
                "temp": false, "_mtime": T1, "poster": "https://img.test/m.jpg",
                "state": {
                    "timeWatched": 0, "timeOffset": 1_800_000, "overallTimeWatched": 0,
                    "timesWatched": 0, "flaggedWatched": 0, "duration": 7_200_000,
                },
            }))
            .unwrap(),
        ];
        let picked = select(&items);
        assert_eq!(
            picked,
            [ContinueItem {
                film: FilmMeta {
                    id: "m".into(),
                    name: "Heat".into(),
                    poster: Some("https://img.test/m.jpg".parse().unwrap()),
                },
                resume: ResumePoint {
                    offset_secs: 1800.0,
                    duration_secs: 7200.0
                },
            }]
        );
        assert_eq!(picked[0].progress(), 0.25);
    }
}
