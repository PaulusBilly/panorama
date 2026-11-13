//! "TMDB titles": the user's own TMDB key, which puts English and original titles on film pages.
use super::{
    Addons, Slot,
    widgets::{View, section},
};
use crate::{assets::Weighted, button::Button, theme::Typography};
use gpui::{Context, Div, Entity, FontWeight, Window, div, prelude::*, px, relative};
use gpui_component::input::{Input, InputState};
use panorama_core::tmdb::TmdbError;

/// The key field and what became of the last save.
pub struct TmdbKey {
    pub input: Entity<InputState>,
    /// The saved key's last four characters, as a hint of which key it is.
    saved: Option<String>,
    busy: bool,
    error: Option<TmdbError>,
}

fn hint(key: &str) -> String {
    key.chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

impl TmdbKey {
    pub fn new(input: Entity<InputState>, saved: Option<&str>) -> Self {
        Self {
            input,
            saved: saved.map(hint),
            busy: false,
            error: None,
        }
    }
}

fn error_copy(error: TmdbError) -> &'static str {
    match error {
        TmdbError::Rejected => "TMDB didn't accept this key. Check it and try again.",
        TmdbError::Network => "Couldn't reach TMDB. Check your connection and try again.",
        TmdbError::Invalid => "Couldn't save the key. Try again.",
    }
}

impl Addons {
    /// Check the typed key with TMDB and keep it, or forget the saved one with `None`.
    pub(super) fn save_tmdb_key(
        &mut self,
        remove: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = (!remove).then(|| self.tmdb.input.read(cx).value().trim().to_owned());
        if self.tmdb.busy || value.as_deref().is_some_and(str::is_empty) {
            return;
        }
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        self.tmdb.busy = true;
        self.tmdb.error = None;
        let hinted = value.as_deref().map(hint);
        let mut job = services.set_tmdb_key(value);
        cx.spawn_in(window, async move |this, cx| {
            let result = (&mut job.0).await.unwrap_or(Err(TmdbError::Invalid));
            let _ = this.update_in(cx, |this, window, cx| {
                this.tmdb.busy = false;
                match result {
                    Ok(()) => {
                        this.tmdb.saved = hinted;
                        this.tmdb
                            .input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                    }
                    Err(error) => this.tmdb.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn tmdb_section(
        &mut self,
        view: View,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let busy = self.tmdb.busy;
        let section = section(
            view,
            "TMDB titles",
            self.tmdb.saved.as_ref().map(|_| "Key saved".into()),
        )
        .child(
            div()
                .mt(px(12.0))
                .max_w(px(560.0))
                .body()
                .text_color(theme.ink_muted)
                .child(
                    "With your own TMDB key, film pages show the English title with the \
                     original or romanized title under it. The key stays on this computer.",
                ),
        );
        let section = match self.tmdb.saved.clone() {
            Some(last) => {
                let focus = self.handle(Slot::TmdbRemove, cx);
                section.child(
                    div()
                        .mt(px(16.0))
                        .min_h(px(48.0))
                        .border_b_1()
                        .border_color(theme.rule)
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(16.0))
                        .child(
                            div()
                                .body()
                                .text_color(theme.ink)
                                .child(format!("Key ending in {last}")),
                        )
                        .child(
                            Button::glass(
                                "tmdb-remove",
                                if busy { "Removing" } else { "Remove key" },
                            )
                            .compact()
                            .busy(busy)
                            .reduced_motion(view.reduced)
                            .focus(&focus, view.keyboard)
                            .tab_stop(view.active)
                            .on_click(cx.listener(
                                |this, _, window, cx| this.save_tmdb_key(true, window, cx),
                            )),
                        ),
                )
            }
            None => {
                let focus = self.handle(Slot::TmdbSave, cx);
                let input_focus = gpui::Focusable::focus_handle(&self.tmdb.input, cx);
                let focused = input_focus.is_focused(window);
                section.child(
                    div()
                        .relative()
                        .mt(px(16.0))
                        .w_full()
                        .min_h(px(48.0))
                        .pb(px(if focused { 9.0 } else { 10.0 }))
                        .border_b(px(if focused { 2.0 } else { 1.0 }))
                        .border_color(theme.ink)
                        .flex()
                        .flex_col()
                        .justify_end()
                        .child(
                            Input::new(&self.tmdb.input)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .aria_label("TMDB API key")
                                .map(|input| gpui::Styled::h(input, px(28.0)))
                                .p_0()
                                .pr(px(120.0))
                                .text_size(px(24.0))
                                .weight(FontWeight::MEDIUM)
                                .line_height(relative(1.15))
                                .text_color(theme.ink),
                        )
                        .child(
                            div().absolute().right_0().bottom(px(4.0)).child(
                                Button::glass("tmdb-save", if busy { "Checking" } else { "Save" })
                                    .compact()
                                    .busy(busy)
                                    .reduced_motion(view.reduced)
                                    .focus(&focus, view.keyboard)
                                    .tab_stop(view.active)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_tmdb_key(false, window, cx)
                                    })),
                            ),
                        ),
                )
            }
        };
        let section = section.child(
            div()
                .mt(px(8.0))
                .caption()
                .text_color(theme.ink_muted)
                .child("Get a free key at themoviedb.org → Settings → API."),
        );
        section.when_some(self.tmdb.error, |section, error| {
            section.child(
                div()
                    .id("tmdb-error")
                    .mt(px(8.0))
                    .label()
                    .text_color(theme.danger)
                    .role(gpui::Role::Alert)
                    .child(error_copy(error)),
            )
        })
    }
}
