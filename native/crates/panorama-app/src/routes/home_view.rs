use super::*;
use crate::{
    film_card::{self, CardProps},
    hero::{self, HeroProps},
    home_layout::Grid,
    theme::{content_width, focus_ring},
};
use gpui::{Div, Render, div, relative, svg};

impl Home {
    fn grid(&mut self, view: ViewSettings, window: &Window, cx: &mut Context<Self>) -> Div {
        let grid = Grid::at(view.width);
        let skeletons = self.films.is_empty()
            && self
                .search
                .as_ref()
                .map_or(self.state.read(cx).loading, |search| search.loading);
        let count = if skeletons { 9 } else { self.films.len() };
        let rows = count.div_ceil(grid.columns);
        let height = (rows as f32 * grid.stride() - if rows > 0 { 36.0 } else { 0.0 }).max(0.0);
        let top = -f32::from(self.scroll.offset().y);
        let origin = self.content_start(view, cx) + if self.search.is_some() { 72.0 } else { 0.0 };
        let start = ((top - origin - 32.0) / grid.stride()).floor().max(0.0) as usize;
        let start = start.saturating_sub(1).min(rows);
        let end = ((top + view.height - origin - 32.0) / grid.stride())
            .ceil()
            .max(0.0) as usize
            + 2;
        let end = end.min(rows);
        let mut result = div()
            .relative()
            .w(px(grid.total_width()))
            .h(px(height))
            .mx_auto()
            .mt(px(32.0));
        let films = self.films.clone();
        let phase = if view.reduced {
            0.0
        } else {
            (Instant::now().duration_since(self.launched).as_secs_f32()
                / SHIMMER.duration.as_secs_f32())
                % 1.0
        };
        for row in start..end {
            let mut tiles = div()
                .absolute()
                .top(px(row as f32 * grid.stride()))
                .left_0()
                .w_full()
                .h(px(grid.height()))
                .flex()
                .gap(px(4.0));
            for index in row * grid.columns..((row + 1) * grid.columns).min(count) {
                let film = films.get(index);
                let (image, poster, unavailable) = if let Some(film) = film {
                    self.artwork(
                        film,
                        (grid.width, grid.height()),
                        view.active
                            && origin + 32.0 + row as f32 * grid.stride() + grid.height() > top
                            && origin + 32.0 + (row as f32) * grid.stride() < top + view.height,
                        window,
                        cx,
                    )
                } else {
                    (None, false, false)
                };
                let focus = self.cards.get(index).unwrap_or(&self.watch);
                let hover = if self.args.hover_first_card && index == 0 {
                    1.0
                } else {
                    self.hovered
                        .get(&index)
                        .map_or(0.0, |t| t.value(Instant::now(), view.reduced))
                };
                tiles = tiles.child(film_card::render(
                    CardProps {
                        film,
                        image,
                        poster,
                        unavailable,
                        focus,
                        hover,
                        index,
                        shimmer: phase,
                    },
                    grid,
                    view,
                    window,
                    cx,
                ));
            }
            result = result.child(tiles);
        }
        self.hovered
            .retain(|index, _| *index >= start * grid.columns && *index < end * grid.columns);
        result
    }
}
impl Render for Home {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self._watch_focus.is_none() {
            self._watch_focus = Some(cx.on_focus(&self.watch, window, |home, window, cx| {
                if -f32::from(home.scroll.offset().y) > home.settings(window, cx).height - 84.0 {
                    home.scroll.set_offset(point(px(0.0), px(0.0)));
                    home.tick(window, cx);
                    cx.notify();
                }
            }));
        }
        let view = self.settings(window, cx);
        let theme = view.theme;
        if self.search.is_none() {
            self.ensure_overlay(view, window, cx);
        }
        if view.active {
            self.images.update(cx, |images, _| images.begin_frame());
        } else {
            self.images.update(cx, |images, _| images.cancel());
        }
        let films = self.films.clone();
        let film = films.first().filter(|_| self.search.is_none());
        let (artwork, poster, unavailable) = if -f32::from(self.scroll.offset().y) < view.height {
            film.map(|f| self.artwork(f, (view.width, view.height), view.active, window, cx))
                .unwrap_or((None, false, true))
        } else {
            (None, false, true)
        };
        let ready = film.is_some() && (unavailable || artwork.is_some());
        if ready != self.ready {
            self.ready = ready;
            self.surface.retarget(
                if ready { 0.0 } else { 1.0 },
                DURATION_DELIBERATE,
                Some(EASE_EDITORIAL),
                Instant::now(),
            );
        }
        if view.active {
            let hero_height = if self.search.is_some() {
                0.0
            } else {
                self.content_start(view, cx)
            };
            let _ = self.shell.update(cx, |shell, cx| {
                shell.update_home_header(
                    self.id,
                    -f32::from(self.scroll.offset().y),
                    hero_height,
                    ready,
                    cx,
                )
            });
        }
        let title_logo = film.and_then(|film| film.logo.as_ref()).is_some_and(|url| {
            !self.images.update(cx, |images, _| {
                images.failed(url.as_str(), (310.0, 128.0), window.scale_factor())
            })
        });
        let logo = film
            .filter(|_| view.active && -f32::from(self.scroll.offset().y) < view.height)
            .and_then(|film| film.logo.as_ref())
            .and_then(|url| {
                let services = self.state.read(cx).services.clone()?;
                self.images.update(cx, |images, cx| {
                    images.request(url.as_str(), (310.0, 128.0), &services, window, cx)
                })
            });
        let hero = hero::render(
            HeroProps {
                film,
                artwork,
                poster,
                logo,
                title_logo,
                overlay: self.overlay.clone(),
                surface: self.surface.value(Instant::now(), view.reduced),
                focus: &self.watch,
                shell: self.shell.clone(),
                hover: self.watch_hover.value(Instant::now(), view.reduced),
            },
            view,
            window,
            cx,
        );
        let state = self.state.read(cx);
        let runtime_error = state.runtime_error.clone();
        let error = self.search.as_ref().map_or_else(
            || state.catalog_error.clone(),
            |search| search.error.clone(),
        );
        let empty = self
            .search
            .as_ref()
            .map_or(state.catalog.is_some() && self.films.is_empty(), |search| {
                !search.loading && search.error.is_none() && self.films.is_empty()
            });
        let more = self.search.is_none() && state.catalog.as_ref().is_some_and(|p| p.has_more);
        let fetching = state.loading_more;
        let mut section = div()
            .w(px(content_width(view.width)))
            .mx_auto()
            .pb(px(112.0));
        if self.search.is_some() {
            section = section.child(self.tabs(view, window));
        }
        if let Some(message) = runtime_error {
            section = section.child(self.alert(message, "Reload Panorama", view, window, cx));
        }
        if let Some(message) = error {
            section = section.child(self.alert(message, "Try again", view, window, cx));
        }
        if empty {
            section = section.child(self.alert(
                self.search.as_ref().map_or_else(
                    || "No popular films are available right now.".into(),
                    |search| format!("No films found for \u{201c}{}\u{201d}.", search.query),
                ),
                if self.search.is_some() {
                    "Clear search"
                } else {
                    "Refresh catalog"
                },
                view,
                window,
                cx,
            ));
        }
        if !self.films.is_empty()
            || self
                .search
                .as_ref()
                .map_or(self.state.read(cx).loading, |search| search.loading)
        {
            section = section.child(self.grid(view, window, cx));
        }
        if more {
            let state = self.state.clone();
            section =
                section.child(
                    div().pt(px(64.0)).flex().justify_center().child(focus_ring(
                        div()
                            .id("catalog-more")
                            .key_context("PanoramaCard")
                            .on_action(cx.listener(|home, _: &NextCard, window, cx| {
                                home.focus_top(window, cx)
                            }))
                            .on_action(cx.listener(|home, _: &PreviousCard, window, cx| {
                                home.focus_card(home.cards.len(), false, window, cx)
                            }))
                            .min_w(px(82.0))
                            .min_h(px(40.0))
                            .px(px(4.0))
                            .border_b_1()
                            .border_color(if fetching { theme.disabled } else { theme.ink })
                            .text_color(if fetching { theme.disabled } else { theme.ink })
                            .text_size(px(13.0))
                            .line_height(relative(1.4))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .role(gpui::Role::Button)
                            .aria_label("Load more")
                            .on_click(move |_, _, cx| {
                                state.update(cx, |state, cx| state.load(true, cx))
                            })
                            .child(if fetching { "Loading" } else { "Load more" }),
                        &self.more,
                        theme,
                        view.active && !fetching,
                        view.keyboard,
                        window,
                    )),
                );
        }
        let container = content_width(view.width);
        let footer = div().w_full().bg(theme.surface).child(
            div()
                .w(px(container))
                .mx_auto()
                .pt(px(200.0 + (view.width * 0.12).clamp(72.0, 128.0)))
                .child(
                    svg()
                        .path("panorama.svg")
                        .w(px(container))
                        .h(px(container * 207.0 / 1223.0))
                        .text_color(gpui::rgb(0xffffff)),
                ),
        );
        if view.active {
            self.images.update(cx, |images, _| images.finish_frame());
        }
        let changing = self.watch_hover.moving(Instant::now(), view.reduced)
            || (!view.reduced && self.images.read(cx).moving())
            || self.surface.moving(Instant::now(), view.reduced)
            || self
                .hovered
                .values()
                .any(|t| t.moving(Instant::now(), view.reduced))
            || self
                .search
                .as_ref()
                .map_or(self.state.read(cx).loading, |search| search.loading)
            || self.args.bench_scroll
            || self.debug_step < self.args.scroll.len()
            || self.args.focus_first_card && !self.debug_focus;
        if changing {
            cx.on_next_frame(window, |home, window, cx| {
                home.tick(window, cx);
                cx.notify();
            });
            window.request_animation_frame();
        }
        div()
            .id("home-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|_, _, window, cx| {
                cx.on_next_frame(window, |home, window, cx| home.tick(window, cx));
                window.request_animation_frame();
            }))
            .when(self.search.is_none(), |root| {
                root.child(
                    div()
                        .pt(px((self.content_start(view, cx) - view.height).max(0.0)))
                        .child(hero),
                )
            })
            .when(self.search.is_some(), |root| {
                root.pt(px(self.content_start(view, cx)))
            })
            .child(section)
            .child(footer)
            .when_some(self.search.as_ref(), |root, search| {
                root.child(crate::a11y::status(
                    "search-status",
                    if search.loading {
                        format!("Searching for {}.", search.query)
                    } else if search.error.is_some() {
                        String::new()
                    } else {
                        format!("{} films found for {}.", self.films.len(), search.query)
                    },
                ))
            })
    }
}
