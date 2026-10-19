use super::home::Home;
use crate::theme::{Typography, focus_ring};
use crate::{app::AppShell, app_state::AppState, args::Args, home_layout::ViewSettings};
use futures::StreamExt;
use gpui::{Context, Div, Entity, FocusHandle, Task, WeakEntity, Window, div, prelude::*, px};
use std::sync::Arc;

pub(crate) struct SearchData {
    pub(crate) query: String,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) request: Option<Task<()>>,
    pub(crate) tab: FocusHandle,
    pub(crate) installed: Vec<panorama_core::stremio::Descriptor>,
    pub(crate) account: crate::app_state::Account,
}

impl Home {
    /// Mount Search using Home's shared cards, grid, alerts and footer.
    pub fn new_search(
        shell: WeakEntity<AppShell>,
        state: Entity<AppState>,
        args: Args,
        id: u64,
        query: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut home = Self::new(shell, state, args, id, cx);
        home.films = Arc::from([]);
        home.cards.clear();
        home.search = Some(SearchData {
            query,
            loading: true,
            error: None,
            request: None,
            tab: cx.focus_handle(),
            installed: vec![],
            account: crate::app_state::Account::SignedOut,
        });
        home.load_search(cx);
        home
    }
    pub(crate) fn load_search(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &mut self.search else {
            return;
        };
        let state = self.state.read(cx);
        search.installed.clone_from(&state.installed);
        search.account = state.account.clone();
        let Some(services) = &state.services else {
            return;
        };
        let (job, mut receiver) = services.search(state.installed.clone(), search.query.clone());
        search.loading = true;
        search.error = None;
        search.request = Some(cx.spawn(async move |entity, cx| {
            let _job = job;
            while let Some(event) = receiver.next().await {
                if entity
                    .update(cx, |home, cx| {
                        let Some(search) = &mut home.search else {
                            return;
                        };
                        search.loading = false;
                        match event {
                            panorama_core::addons::ResourceEvent::CacheHit(films)
                            | panorama_core::addons::ResourceEvent::Fresh(films) => {
                                home.films = films
                                    .into_iter()
                                    .filter(|film| !film.name.trim().is_empty())
                                    .collect::<Vec<_>>()
                                    .into();
                                home.cards.truncate(home.films.len());
                                while home.cards.len() < home.films.len() {
                                    home.cards.push(cx.focus_handle());
                                }
                            }
                            panorama_core::addons::ResourceEvent::Failed(kind) => {
                                search.error = Some(
                                    match kind {
                                        panorama_core::addons::FailureKind::Offline => {
                                            "Can't search films. Check your connection."
                                        }
                                        panorama_core::addons::FailureKind::Timeout => {
                                            "Searching films took too long. Try again."
                                        }
                                        _ => "Could not search films. Try again.",
                                    }
                                    .into(),
                                )
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        cx.notify();
    }
    pub(crate) fn content_start(&self, view: ViewSettings, cx: &Context<Self>) -> f32 {
        let header = self.shell.upgrade().map_or(60.0, |shell| {
            shell.read(cx).search_header_height(view.width)
        });
        if self.search.is_some() {
            header
        } else {
            view.height
                + self
                    .shell
                    .upgrade()
                    .map_or(0.0, |shell| shell.read(cx).home_search_padding(view.width))
        }
    }
    pub(crate) fn tabs(&self, view: ViewSettings, window: &Window) -> Div {
        let Some(search) = &self.search else {
            return div();
        };
        div().child(
            div()
                .id("search-tabs")
                .role(gpui::Role::TabList)
                .aria_label("Search result categories")
                .pt(px(32.0))
                .border_b_1()
                .border_color(view.theme.rule)
                .flex()
                .gap(px(32.0))
                .child(focus_ring(
                    div()
                        .id("films-tab")
                        .role(gpui::Role::Tab)
                        .aria_label("Films")
                        .aria_selected(true)
                        .min_h(px(40.0))
                        .px(px(4.0))
                        .mb(px(-1.0))
                        .border_b_2()
                        .border_color(view.theme.ink)
                        .label()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .child("FILMS"),
                    &search.tab,
                    view.theme,
                    view.active,
                    view.keyboard,
                    window,
                )),
        )
    }
}
