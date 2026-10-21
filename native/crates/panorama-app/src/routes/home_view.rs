use super::*;
use crate::{
    button::Button,
    hero::{self, HeroProps},
};
use gpui::{Div, Render, div};

impl Home {
    fn grid(&mut self, view: ViewSettings, window: &Window, cx: &mut Context<Self>) -> Div {
        let skeletons = self.state.read(cx).loading;
        let top = -f32::from(self.scroll.offset().y);
        // The grid follows the full-window hero.
        let origin = view.height;
        let opacity =
            self.switcher
                .grid_opacity(self.grid.films.is_empty(), Instant::now(), view.reduced);
        let offset = origin + self.lead(cx) - top;
        let services = self.state.read(cx).services.clone();
        let images = self.images.clone();
        let mt = if self.switcher_visible(cx) { 0.0 } else { 32.0 };
        self.grid
            .render_rows(
                offset,
                skeletons,
                view,
                &images,
                services.as_ref(),
                window,
                cx,
            )
            .mt(px(mt))
            .opacity(opacity)
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
        self.advance_swap(view.reduced, cx);
        if view.active {
            self.images.update(cx, |images, _| images.begin_frame());
        } else {
            self.images.update(cx, |images, _| images.cancel());
        }
        let films = self.grid.films.clone();
        let film = films.first();
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
                surface: self.surface.value(Instant::now(), view.reduced),
                focus: &self.watch,
                shell: self.shell.clone(),
            },
            view,
            window,
            cx,
        );
        let state = self.state.read(cx);
        let runtime_error = state.runtime_error.clone();
        let error = state.catalog_error.clone();
        let empty = state.catalog.is_some() && self.grid.films.is_empty();
        let more = state.catalog.as_ref().is_some_and(|p| p.has_more);
        let fetching = state.loading_more;
        let mut section = div()
            .ml(px(view.column().left))
            .w(px(view.column().width))
            .pb(px(112.0));
        if self.switcher_visible(cx) {
            section = section.child(self.catalog_row(view, window, cx));
        }
        if let Some(message) = runtime_error {
            section = section.child(self.alert(message, "Reload Panorama", view));
        }
        if let Some(message) = error {
            section = section.child(self.alert(message, "Try again", view));
        }
        if empty {
            section = section.child(self.alert(
                "No popular films are available right now.".into(),
                "Refresh catalog",
                view,
            ));
        }
        if !self.grid.films.is_empty() || self.state.read(cx).loading {
            section = section.child(self.grid(view, window, cx));
        }
        if more {
            let state = self.state.clone();
            section = section.child(
                div()
                    .pt(px(64.0))
                    .flex()
                    .justify_center()
                    .key_context("PanoramaCard")
                    .on_action(
                        cx.listener(|home, _: &NextCard, window, cx| home.focus_top(window, cx)),
                    )
                    .on_action(cx.listener(|home, _: &PreviousCard, window, cx| {
                        home.step_card(home.grid.len(), false, window, cx)
                    }))
                    .child(
                        Button::glass(
                            "catalog-more",
                            if fetching { "Loading" } else { "Load more" },
                        )
                        .compact()
                        .busy(fetching)
                        .reduced_motion(view.reduced)
                        .focus(&self.more, view.keyboard)
                        .tab_stop(view.active)
                        .on_click(move |_, _, cx| {
                            state.update(cx, |state, cx| state.load(true, cx))
                        }),
                    ),
            );
        }
        let footer = crate::footer::footer(theme, view.width);
        if view.active {
            self.images.update(cx, |images, _| images.finish_frame());
        }
        let changing = self.switcher.moving(Instant::now(), view.reduced)
            || (!view.reduced && self.images.read(cx).moving())
            || self.surface.moving(Instant::now(), view.reduced)
            || self.grid.moving(view.reduced)
            || self.state.read(cx).loading
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
                // request_animation_frame panics outside render; notify schedules the frame instead.
                cx.notify();
            }))
            .child(hero)
            .child(section)
            .child(footer)
    }
}
