//! A show's seasons and episodes, the first section under its hero.
use super::{
    credits::{heading, rail},
    *,
};
use crate::assets::Weighted;
use crate::{
    button::Icon,
    hero, tab_strip,
    theme::{
        Theme, Typography, caps, focus_ring,
        scrim::{Edge, scrim},
    },
};
use crate::{
    theme::{DURATION_STANDARD, EASE_EDITORIAL},
    transition::bezier,
};
use gpui::{Animation, AnimationExt, Div, FontWeight, div, prelude::*, px, relative, svg};
use panorama_core::addons::{Episode, next_after, today};
use std::{collections::HashSet, ops::Range};

/// A season past this many episodes is split, since one rail cannot hold a long anime.
const TAB_LIMIT: usize = 100;
/// Height over width: the still, then the copy overlapping its lower edge.
const CARD_RATIO: f32 = 0.74;
/// How dark the shade under the words gets at the bottom edge.
const SHADE: f32 = 0.7;
const COPY_PAD: f32 = 16.0;
/// Room for the copy at its longest, so every card's eyebrow sits at the same height.
const COPY_HEIGHT: f32 = 120.0;

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
        .or_else(|| episodes.iter().find(|ep| !unaired(ep, today)))
}

fn unaired(episode: &Episode, today: &str) -> bool {
    episode.released.as_deref().is_some_and(|date| date > today)
}

/// The name an episode goes by on buttons.
pub(super) fn code(episode: &Episode) -> String {
    format!("S{} E{}", episode.season, episode.number)
}

impl Film {
    /// Replace the episodes; the rest of the page keeps reading them from here, so a render
    /// does not copy thousands of them.
    pub(super) fn set_episodes(&mut self, mut episodes: Vec<Episode>) {
        if let Some(tmdb) = &self.tmdb_episodes {
            panorama_core::tmdb::enrich(&mut episodes, tmdb);
        }
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
        let play = crate::party_prompt::Play::Launch {
            film: Box::new(film),
            video_id: episode.id,
            resume,
        };
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.play_or_ask(play, window, cx));
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
        // As wide as a Home card, so a show's episodes read at the same size as the films.
        let grid = crate::home_layout::Grid::fit(view.column());
        let (tile, pitch) = (grid.width, grid.pitch());
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
            tab_strip::strip("film-seasons", "Seasons", col.width, theme, labels)
        });
        Some(
            div()
                .flex()
                .flex_col()
                .child(match strip {
                    Some(strip) => div().ml(px(col.left)).mb(px(16.0)).child(strip),
                    None => heading("Episodes", theme).ml(px(col.left)).w(px(col.width)),
                })
                .child({
                    let row = rail(
                        "film-episodes",
                        cards,
                        grid.gap,
                        false,
                        // The words are part of the card, so the arrows centre on all of it.
                        Some(card_height(tile)),
                        &self.episode_rail,
                        view,
                        |film| &mut film.episode_rail,
                        cx,
                    );
                    // A season picked by hand fades its episodes in.
                    if self.tab.is_some() && !view.reduced {
                        row.with_animation(
                            ("film-season", selected),
                            Animation::new(DURATION_STANDARD)
                                .with_easing(|t| bezier(t, EASE_EDITORIAL)),
                            |row, t| row.opacity(t),
                        )
                        .into_any_element()
                    } else {
                        row.into_any_element()
                    }
                }),
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
        let height = card_height(width);
        let unaired = unaired(episode, today);
        let backdrop = self.metadata.as_ref().and_then(|m| m.background.as_ref());
        let image = episode
            .thumbnail
            .as_ref()
            .or(backdrop)
            .filter(|_| near)
            .and_then(|url| self.fading(url, (width, height), view, window, cx));
        let resume = self.resume.as_ref().filter(|point| {
            point.video_id.as_ref() == Some(&episode.id) && point.duration_secs > 0.0
        });
        let watched = self.watched.contains(&episode.id);
        let group: gpui::SharedString = format!("film-episode-{index}").into();
        let mut card = div()
            .relative()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(theme.surface);
        if let Some(image) = image {
            let fade = crate::film_card::fade_in(&image, view.reduced);
            card = card.child(hero::cover(image.image, (width, height), false, 1.0).opacity(fade));
        }
        // The still is baked soft toward the bottom; this shade only lifts the words off it.
        let card = card
            .child(scrim(theme, Edge::Bottom, relative(0.62), SHADE))
            .when(watched && resume.is_none(), |card| {
                card.child(
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
            // Pointing at an aired episode lifts its still a little.
            .when(!unaired, |card| {
                card.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(theme.ink.opacity(0.06))
                        .opacity(0.0)
                        .group_hover(group.clone(), |style| style.opacity(1.0)),
                )
            })
            .child(self.episode_copy(episode, unaired, resume, height - COPY_HEIGHT, theme))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_1()
                    .border_color(theme.image_outline),
            );
        let mut tile = div()
            .relative()
            .group(group)
            .w(px(width))
            .h(px(height))
            .when(unaired, |tile| tile.opacity(0.5))
            .child(card);
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
                .inset_0(),
            );
        }
        tile
    }

    /// The words laid over a card's lower half: which episode, its name, what happens, and
    /// what pressing it does.
    fn episode_copy(
        &self,
        episode: &Episode,
        unaired: bool,
        resume: Option<&panorama_core::stremio::ResumePoint>,
        top: f32,
        theme: Theme,
    ) -> Div {
        let kind = if episode.season == 0 {
            "Special"
        } else {
            "Episode"
        };
        let eyebrow = if episode.numbered {
            format!("{kind} {}", episode.number)
        } else {
            kind.to_owned()
        };
        let overview = episode.overview.clone().filter(|_| !unaired);
        // What is left to know before pressing play, after the number: when it airs, or how
        // much is left of it.
        let note = match (unaired, resume) {
            (true, _) => Some(
                episode
                    .released
                    .as_deref()
                    .map(|date| format!("Airs {date}"))
                    .unwrap_or_else(|| "Coming soon".to_owned()),
            ),
            (false, Some(point)) => {
                crate::film_display::format_time_left(point.offset_secs, Some(point.duration_secs))
                    .map(|left| format!("{left} left"))
            }
            (false, None) => None,
        };
        let eyebrow = match note {
            Some(note) => format!("{eyebrow} · {note}"),
            None => eyebrow,
        };
        // Along the card's bottom edge, as on a Continue card.
        let progress = resume.map(|point| {
            let share = (point.offset_secs / point.duration_secs).clamp(0.0, 1.0) as f32;
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(3.0))
                .bg(theme.progress_track)
                .child(div().h_full().w(relative(share)).bg(theme.progress_fill))
        });
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(top))
            .bottom_0()
            // Set like a Home card's caption: the note over the name, both in capitals.
            .px(px(20.0))
            .pt(px(COPY_PAD))
            .pb(px(16.0))
            .flex()
            .flex_col()
            .text_color(theme.ink)
            .child(
                div()
                    .text_size(px(14.0))
                    .weight(FontWeight::NORMAL)
                    .line_height(relative(1.15))
                    .text_color(theme.ink.opacity(0.82))
                    .truncate()
                    .child(caps(&eyebrow)),
            )
            .child(
                div()
                    .text_size(px(22.0))
                    .weight(FontWeight::MEDIUM)
                    .line_height(relative(1.05))
                    .mt(px(2.0))
                    .truncate()
                    .child(caps(&episode.title)),
            )
            .when_some(overview, |copy, overview| {
                copy.child(
                    div()
                        .label()
                        .font_weight(gpui::FontWeight::NORMAL)
                        .mt(px(6.0))
                        .text_color(theme.ink.opacity(0.78))
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(overview),
                )
            })
            .children(progress)
    }
}

/// A card is its still plus room beneath for two lines of description.
fn card_height(width: f32) -> f32 {
    (width * CARD_RATIO).round()
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
            runtime: None,
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
        let mut coming = ep(1, 1);
        coming.released = Some("2099-01-01".into());
        assert_eq!(id(target(&[coming], None, &none, at)), None);
    }
}
