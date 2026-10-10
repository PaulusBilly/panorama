//! Home placeholder and temporary navigation links.
use crate::{
    app::AppShell,
    router::Route,
    theme::{Typography, content_width, focus_ring},
};
use gpui::{Context, FocusHandle, Render, ScrollHandle, WeakEntity, Window, div, prelude::*, px};

/// Home placeholder with temporary links for exercising navigation.
pub struct Home {
    shell: WeakEntity<AppShell>,
    id: u64,
    links: [FocusHandle; 4],
    scroll: ScrollHandle,
}

impl Home {
    /// Allocate a route instance with its own scroll and focus.
    pub fn new(shell: WeakEntity<AppShell>, id: u64, cx: &mut Context<Self>) -> Self {
        Self {
            shell,
            id,
            links: std::array::from_fn(|_| cx.focus_handle()),
            scroll: ScrollHandle::new(),
        }
    }
}

impl Render for Home {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width: f32 = window.viewport_size().width.into();
        let (theme, active, keyboard_navigation) = self
            .shell
            .upgrade()
            .map(|shell| {
                (
                    shell.read(cx).theme,
                    shell.read(cx).is_current(self.id),
                    shell.read(cx).keyboard_navigation,
                )
            })
            .unwrap_or_default();
        let routes = [
            (
                "Search",
                Route::Search {
                    query: String::new(),
                },
            ),
            (
                "Film",
                Route::Film {
                    id: "tt0111161".into(),
                },
            ),
            (
                "Player",
                Route::Player {
                    id: "tt0111161".into(),
                },
            ),
            ("Addons", Route::Addons),
        ];
        let links = routes
            .into_iter()
            .enumerate()
            .map(|(index, (label, route))| {
                let shell = self.shell.clone();
                focus_ring(
                    div()
                        .id(("route-link", index))
                        .label()
                        .min_h(px(44.0))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .role(gpui::Role::Link)
                        .aria_label(label)
                        .hover(|style| style.bg(theme.hover))
                        .on_click(move |_, window, cx| {
                            let _ = shell
                                .update(cx, |shell, cx| shell.navigate(route.clone(), window, cx));
                        })
                        .child(label),
                    &self.links[index],
                    theme,
                    active,
                    keyboard_navigation,
                    window,
                )
            })
            .collect::<Vec<_>>();
        div()
            .id("home-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .w(px(content_width(width)))
                    .mx_auto()
                    .pt(px(48.0))
                    .child(div().title().child("Home"))
                    .child(
                        div()
                            .body()
                            .mt(px(12.0))
                            .child("Discover films in Panorama."),
                    )
                    .child(div().display(width).mt(px(32.0)).child("Popular films"))
                    .child(div().flex().gap(px(16.0)).mt(px(20.0)).children(links)),
            )
    }
}
