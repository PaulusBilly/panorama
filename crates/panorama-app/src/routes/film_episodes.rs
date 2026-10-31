//! A show's seasons and episodes, the first section under its hero.
use super::{
    credits::{columns, heading, rail, span},
    *,
};
use crate::{
    button::Icon,
    hero,
    home_layout::TILE_RATIO,
    tab_strip,
    theme::{Typography, focus_ring},
};
use gpui::{Div, div, prelude::*, px, svg};
use panorama_core::addons::{Episode, next_after, today};
use std::{collections::HashSet, ops::Range};

/// A season past this many episodes is split, since one rail cannot hold a long anime.
const TAB_LIMIT: usize = 100;
const TILE_MIN: f32 = 240.0;
const TILE_GAP: f32 = 6.0;
const TILES: usize = 4;

/// One stop on the strip: a season, or a stretch of a long one.
#[derive(Debug, PartialEq)]
pub(super) struct Tab {
    pub label: String,
    pub range: Range<usize>,
}

pub(super) fn tabs(episodes: &[Episode]) -> Vec<Tab> {
    let seasons = episodes.chunk_by(|a, b| a.season == b.season).count();
    let mut tabs = vec![];
    let mut start = 0;
    for season in episodes.chunk_by(|a, b| a.season == b.season) {
        let name = match season[0].season {
            0 => "Specials".to_owned(),
            number => format!("Season {number}"),
        };
        if season.len() <= TAB_LIMIT {
            tabs.push(Tab {
                label: name,
                range: start..start + season.len(),
            });
        } else {
            for (index, part) in season.chunks(TAB_LIMIT).enumerate() {
                let (first, last) = (part[0].number, part[part.len() - 1].number);
                let from = start + index * TAB_LIMIT;
                tabs.push(Tab {
                    label: if seasons > 1 {
                        format!("S{} {first}-{last}", season[0].season)
                    } else {
                        format!("{first}-{last}")
                    },
                    range: from..from + part.len(),
                });
            }
        }
        start += season.len();
    }
    tabs
}

/// What Play plays: the episode in progress, else the one after the last watched, else the first.
pub(super) fn target<'a>(
    episodes: &'a [Episode],
    resume: Option<&str>,
    watched: &HashSet<String>,
    today: &str,
) -> Option<&'a Episode> {
    if let Some(resuming) = resume.and_then(|id| episodes.iter().find(|ep| ep.id == id)) {
        return Some(resuming);
    }
    episodes
        .iter()
        .rfind(|ep| ep.season != 0 && watched.contains(&ep.id))
        .and_then(|last| next_after(episodes, &last.id, today))
        .or(episodes.first())
}

/// The name an episode goes by on buttons.
pub(super) fn code(episode: &Episode) -> String {
    format!("S{} E{}", episode.season, episode.number)
}

impl Film {
    /// Replace the episodes; the rest of the page keeps reading them from here, so a render
    /// does not copy thousands of them.
    pub(super) fn set_episodes(&mut self, episodes: Vec<Episode>) {
        self.tabs = tabs(&episodes);
        self.episodes = Arc::new(episodes);
        self.tab_focus
            .truncate(self.tabs.len().min(self.tab_focus.len()));
    }

    pub(super) fn target_episode(&self) -> Option<&Episode> {
        let id = self.target.as_deref()?;
        self.episodes.iter().find(|ep| ep.id == id)
    }

    /// Choose what Play plays, and look up its sources if that changed.
    pub(super) fn sync_target(&mut self, cx: &mut Context<Self>) {
        if self.kind == Kind::Movie || self.loading {
            return;
        }
        let resume = self.resume.as_ref().and_then(|p| p.video_id.as_deref());
        let next = target(&self.episodes, resume, &self.watched, &today()).map(|ep| ep.id.clone());
        if next == self.target {
            self.sources_loading = self.sources_loading && next.is_some();
            return;
        }
        self.target = next;
        if self.target.is_some() {
            self.load_sources(false, cx);
        } else {
            self.sources_loading = false;
        }
    }

    /// Read which episodes are watched, then settle what Play plays.
    pub(super) fn load_watched(&mut self, cx: &mut Context<Self>) {
        if self.kind == Kind::Movie {
            return;
        }
        let session = self
            .state
            .read(cx)
            .services
            .clone()
            .and_then(|services| Some((services.session.clone()?, services)));
        let Some((session, services)) = session else {
            if self.args.fixtures {
                self.watched = crate::fixtures::watched(&self.episodes);
            }
            self.sync_target(cx);
            return;
        };
        let id = self.id.clone();
        let episodes = self.episodes.clone();
        let mut job = Job(services
            .runtime
            .spawn(async move { session.lock().await.watched_episodes(&id, &episodes) }));
        self.watched_task = Some(cx.spawn(async move |entity, cx| {
            let watched = (&mut job.0).await.unwrap_or_default();
            let _ = entity.update(cx, |film, cx| {
                film.watched = watched;
                film.sync_target(cx);
                cx.notify();
            });
        }));
    }

    /// Clicking an episode plays it; only Play's own episode has its sources ready.
    fn play_episode(&mut self, episode: Episode, window: &mut Window, cx: &mut Context<Self>) {
        let playing_target = self.target.as_ref() == Some(&episode.id);
        if !matches!(self.account, Account::SignedIn(_))
            || (playing_target && self.selected.is_some())
        {
            return self.primary_action(window, cx);
        }
        let Some(mut film) = self.metadata.clone() else {
            return;
        };
        film.episodes = (*self.episodes).clone();
        let resume = self
            .resume
            .clone()
            .filter(|point| point.video_id.as_ref() == Some(&episode.id));
        self.state
            .update(cx, |state, cx| state.launch(film, episode.id, resume, cx));
        let route = crate::router::Route::Player {
            id: self.id.clone(),
        };
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.navigate(route, window, cx));
    }

    pub(super) fn episodes_section(
        &mut self,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if self.tabs.is_empty() {
            return None;
        }
        let theme = view.theme;
        let col = view.column();
        let episodes = self.episodes.clone();
        let target_index = self
            .target
            .as_ref()
            .and_then(|id| episodes.iter().position(|ep| ep.id == *id));
        let selected = self.tab.unwrap_or_else(|| {
            target_index
                .and_then(|index| self.tabs.iter().position(|tab| tab.range.contains(&index)))
                .unwrap_or(0)
        });
        let range = self.tabs.get(selected)?.range.clone();
        self.tab_focus
            .resize_with(self.tabs.len(), || cx.focus_handle());
        self.episode_focus
            .resize_with(range.len(), || cx.focus_handle());
        let tile = span(
            col.width,
            columns(col.width, TILE_MIN, TILE_GAP, TILES),
            TILE_GAP,
        );
        let pitch = tile + TILE_GAP;
        self.episode_rail.settle(pitch, view.reduced);
        let focused = self.episode_focus[..range.len()]
            .iter()
            .position(|focus| focus.is_focused(window));
        let arrived = target_index
            .filter(|index| range.contains(index) && self.tab.is_none())
            .map(|index| index - range.start)
            .filter(|_| f32::from(self.episode_rail.scroll.max_offset().x) > 0.0);
        if let Some(index) = focused.or(arrived) {
            self.episode_rail.follow(index, pitch);
        }
        let today = today();
        let position = self.episode_rail.position();
        let cards = episodes[range]
            .iter()
            .enumerate()
            .map(|(index, episode)| {
                let x = col.left + index as f32 * pitch - position;
                let near = x + tile > 0.0 && x < view.width;
                self.episode_tile(episode, index, tile, near, &today, view, window, cx)
                    .flex_shrink_0()
                    .into_any_element()
            })
            .collect();
        let strip = (self.tabs.len() > 1).then(|| {
            let labels = self.tabs.iter().enumerate().map(|(index, tab)| {
                tab_strip::tab(
                    ("film-season", index),
                    &tab.label,
                    index == selected,
                    &self.tab_focus[index],
                    view,
                    window,
                )
                .on_click(cx.listener(move |film, _, _, cx| film.pick_tab(index, cx)))
            });
            tab_strip::strip("film-seasons", "Seasons", col.width, labels)
        });
        Some(
            div()
                .flex()
                .flex_col()
                .child(match strip {
                    Some(strip) => div().ml(px(col.left)).mb(px(16.0)).child(strip),
                    None => heading("Episodes", theme).ml(px(col.left)).w(px(col.width)),
                })
                .child(rail(
                    "film-episodes",
                    cards,
                    TILE_GAP,
                    false,
                    &self.episode_rail,
                    view,
                    cx.listener(|film, _: &gpui::ScrollWheelEvent, _, cx| {
                        film.episode_rail.wheel();
                        cx.notify();
                    }),
                )),
        )
    }

    fn pick_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        self.tab = Some(index);
        self.episode_rail.scroll.set_offset(gpui::Point::default());
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    fn episode_tile(
        &self,
        episode: &Episode,
        index: usize,
        width: f32,
        near: bool,
        today: &str,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let height = (width * TILE_RATIO).round();
        let unaired = episode.released.as_deref().is_some_and(|date| date > today);
        let backdrop = self.metadata.as_ref().and_then(|m| m.background.as_ref());
        let image = episode
            .thumbnail
            .as_ref()
            .or(backdrop)
            .filter(|_| near)
            .and_then(|url| self.image(url, (width, height), view, window, cx).0);
        let progress = self
            .resume
            .as_ref()
            .filter(|point| point.video_id.as_ref() == Some(&episode.id))
            .map(|point| (point.offset_secs / point.duration_secs) as f32);
        let watched = self.watched.contains(&episode.id);
        let mut still = div()
            .relative()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(theme.surface);
        if let Some(image) = image {
            let fade = crate::film_card::fade_in(&image, view.reduced);
            still =
                still.child(hero::cover(image.image, (width, height), false, 1.0).opacity(fade));
        }
        let still = still
            .when_some(progress, |still, share| {
                still.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .h(px(3.0))
                        .w(px(width * share.clamp(0.0, 1.0)))
                        .bg(theme.ink),
                )
            })
            .when(watched && progress.is_none(), |still| {
                still.child(
                    div()
                        .absolute()
                        .top(px(10.0))
                        .right(px(10.0))
                        .size(px(24.0))
                        .rounded_full()
                        .bg(theme.scrim.opacity(0.6))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path(Icon::Check.path())
                                .size(px(14.0))
                                .text_color(theme.ink),
                        ),
                )
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_1()
                    .border_color(theme.image_outline),
            );
        let name = format!("{}  {}", episode.number, episode.title);
        let note = if unaired {
            episode
                .released
                .as_deref()
                .map(|date| format!("Airs {date}"))
        } else {
            episode.overview.clone()
        };
        let mut tile = div()
            .relative()
            .w(px(width))
            .flex()
            .flex_col()
            .when(unaired, |tile| tile.opacity(0.5))
            .child(still)
            .child(
                div()
                    .label()
                    .mt(px(10.0))
                    .text_color(theme.ink)
                    .line_clamp(1)
                    .child(name),
            )
            .when_some(note, |tile, note| {
                tile.child(
                    div()
                        .caption()
                        .mt(px(4.0))
                        .text_color(theme.ink_muted)
                        .line_clamp(2)
                        .child(note),
                )
            });
        if !unaired && let Some(focus) = self.episode_focus.get(index) {
            let playing = episode.clone();
            tile = tile.child(
                focus_ring(
                    div()
                        .id(("film-episode", index))
                        .size_full()
                        .cursor_pointer()
                        .role(gpui::Role::Button)
                        .aria_label(format!("Play {}, {}", code(episode), episode.title))
                        .on_click(cx.listener(move |film, _, window, cx| {
                            film.play_episode(playing.clone(), window, cx)
                        })),
                    focus,
                    theme,
                    view.active,
                    view.keyboard,
                    0.0,
                    window,
                )
                .absolute()
                .top_0()
                .left_0()
                .w(px(width))
                .h(px(height)),
            );
        }
        tile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(season: u32, number: u32) -> Episode {
        Episode {
            id: format!("s{season}e{number}"),
            season,
            number,
            numbered: true,
            title: String::new(),
            overview: None,
            thumbnail: None,
            released: None,
        }
    }

    #[test]
    fn seasons_become_tabs_with_specials_named() {
        let episodes = [ep(1, 1), ep(1, 2), ep(2, 1), ep(0, 1)];
        assert_eq!(
            tabs(&episodes),
            [
                Tab {
                    label: "Season 1".into(),
                    range: 0..2
                },
                Tab {
                    label: "Season 2".into(),
                    range: 2..3
                },
                Tab {
                    label: "Specials".into(),
                    range: 3..4
                },
            ]
        );
    }

    #[test]
    fn a_long_season_splits_into_stretches() {
        let episodes: Vec<_> = (1..=230).map(|number| ep(1, number)).collect();
        let labels: Vec<_> = tabs(&episodes).into_iter().map(|tab| tab.label).collect();
        assert_eq!(labels, ["1-100", "101-200", "201-230"]);
        let mut two = episodes;
        two.push(ep(2, 1));
        assert_eq!(tabs(&two)[0].label, "S1 1-100");
        assert_eq!(tabs(&two)[1].range, 100..200);
    }

    #[test]
    fn play_resumes_then_moves_on_then_starts_over() {
        let episodes = [ep(1, 1), ep(1, 2), ep(2, 1), ep(0, 1)];
        let none = HashSet::new();
        let id = |found: Option<&Episode>| found.map(|ep| ep.id.clone());
        let at = "2026-01-01";
        assert_eq!(id(target(&episodes, None, &none, at)), Some("s1e1".into()));
        let seen = HashSet::from(["s1e1".to_owned(), "s1e2".to_owned()]);
        assert_eq!(id(target(&episodes, None, &seen, at)), Some("s2e1".into()));
        assert_eq!(
            id(target(&episodes, Some("s1e2"), &seen, at)),
            Some("s1e2".into())
        );
        let all = HashSet::from(["s2e1".to_owned()]);
        assert_eq!(id(target(&episodes, None, &all, at)), Some("s1e1".into()));
        assert_eq!(id(target(&[], None, &none, at)), None);
    }
}
