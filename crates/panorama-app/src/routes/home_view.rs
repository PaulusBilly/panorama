use super::lineup::lineup;
use super::*;
use crate::{
    button::Button,
    continue_row::ContinueRow,
    hero::{self, HeroState},
};
use gpui::{Div, Render, div};

impl Home {
    fn first_viewport(
        &mut self,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let signed_in = self.signed_in(cx);
        let cards = self.state.read(cx).continue_watching.read(cx).cards();
        let quiet = self.state.read(cx).quiet.clone();
        self.grid.quiet = quiet.clone();
        self.row.quiet = quiet;
        let films = self.grid.films.clone();
        let lineup = lineup(signed_in, &cards, &films);
        self.row.sync(lineup.row.len(), cx);
        let row_height = (!lineup.row.is_empty()).then(|| ContinueRow::height(view));
        self.fold = hero::fold(view, row_height);
        let fold = self.fold;
        let hero_height = hero::height(view);
        // The artwork fades out midway between the hero actions and the Continue heading.
        let art_height = hero_height - hero::ROW_GAP / 2.0;
        let visible = view.active && -f32::from(self.scroll.offset().y) < fold;
        let film = lineup.hero.map(|hero| hero.film().clone());
        self.track_saved(film.as_ref().map(|film| film.id.as_str()), cx);

        let art = film.as_ref().and_then(|film| {
            let (image, poster, _) =
                self.artwork(film, (view.width, art_height), visible, window, cx);
            image.map(|image| (image, poster))
        });
        let services = self.state.read(cx).services.clone();
        let logo_url = film
            .as_ref()
            .and_then(|film| film.logo.clone())
            .filter(|url| {
                !self.images.update(cx, |images, _| {
                    images.failed(url.as_str(), hero::LOGO, window.scale_factor())
                })
            });
        let logo = logo_url.as_ref().filter(|_| visible).and_then(|url| {
            let services = services.clone()?;
            self.images.update(cx, |images, cx| {
                images.request(url.as_str(), hero::LOGO, &services, window, cx)
            })
        });
        let mut content = div().absolute().inset_0().flex().flex_col();
        if let Some(featured) = lineup.hero {
            let state = HeroState {
                logo_pending: logo_url.is_some() && logo.is_none(),
                logo,
                signed_in,
                saved: self.saved,
                saving: self.save_task.is_some(),
            };
            let block = hero::render(featured, state, &self.hero, view, window, cx)
                .ml(px(view.column().left));
            content = content.child(
                div()
                    .h(px(hero_height))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .pb(px(hero::ROW_GAP))
                    .child(block),
            );
        } else {
            content = content.child(div().h(px(hero_height)).flex_shrink_0());
        }
        if row_height.is_some() {
            content = content.child(self.row.render(
                lineup.row,
                view,
                &self.images,
                services.as_ref(),
                visible,
                window,
                cx,
            ));
        }
        div()
            .id("featured-film")
            .relative()
            .w_full()
            .h(px(fold))
            .flex_shrink_0()
            .overflow_hidden()
            .aria_label("Featured film")
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(art_height))
                    .child(hero::backdrop(art, (view.width, art_height), view.reduced))
                    .child(hero::scrims(view.theme, view.column(), view.width)),
            )
            .child(content)
    }
    fn grid(&mut self, view: ViewSettings, window: &Window, cx: &mut Context<Self>) -> Div {
        let skeletons = self.state.read(cx).loading;
        let top = -f32::from(self.scroll.offset().y);
        let origin = self.fold;
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
            self._watch_focus =
                Some(cx.on_focus(&self.hero.primary, window, |home, window, cx| {
                    if -f32::from(home.scroll.offset().y) > 0.0 {
                        home.scroll.set_offset(point(px(0.0), px(0.0)));
                        home.tick(window, cx);
                        cx.notify();
                    }
                }));
        }
        let view = self.settings(window, cx);
        self.advance_swap(view.reduced, cx);
        if view.active {
            self.images.update(cx, |images, _| images.begin_frame());
        } else {
            self.images.update(cx, |images, _| images.cancel());
        }
        let first = self.first_viewport(view, window, cx);
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
        if view.active {
            self.images.update(cx, |images, _| images.finish_frame());
        }
        let changing = self.switcher.moving(Instant::now(), view.reduced)
            || (!view.reduced && self.images.read(cx).moving())
            || self.grid.moving(view.reduced)
            || self.row.moving(view.reduced)
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
            // Sideways swipes belong to the Continue row; without this the page turns them
            // into vertical scrolling.
            .restrict_scroll_to_axis()
            .on_scroll_wheel(cx.listener(|_, _, window, cx| {
                cx.on_next_frame(window, |home, window, cx| home.tick(window, cx));
                // request_animation_frame panics outside render; notify schedules the frame instead.
                cx.notify();
            }))
            .child(first)
            .child(section)
            .child(crate::home_layout::footer())
    }
}
