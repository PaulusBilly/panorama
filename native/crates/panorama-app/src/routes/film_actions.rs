use super::*;
use crate::{
    button::{Button, Icon},
    film_display::Primary,
};
use gpui::{Div, div, prelude::*, px};

impl Film {
    pub(super) fn primary_action(
        &mut self,
        from_start: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.account, Account::SignedIn(_)) {
            self.sign_in(window, cx);
            return;
        }
        if let (Some(source), Some(film)) = (self.selected, self.metadata.clone()) {
            let id = self.id.clone();
            let selection = crate::playback::selection::PlaybackSelection {
                film,
                groups: self.groups.clone(),
                source,
                start: self
                    .resume
                    .filter(|_| !from_start)
                    .map_or(crate::playback::selection::StartMode::Restart, |point| {
                        crate::playback::selection::StartMode::Resume(point.offset_secs)
                    }),
                meta_base: self.meta_base.clone(),
            };
            self.state
                .update(cx, |state, _| state.playback = Some(selection));
            let _ = self.shell.update(cx, |shell, cx| {
                shell.navigate(crate::router::Route::Player { id }, window, cx)
            });
        }
    }
    pub(super) fn actions(&mut self, view: ViewSettings, cx: &mut Context<Self>) -> Div {
        let primary = Primary::at(
            matches!(self.account, Account::SignedIn(_)),
            self.sources_loading,
            self.selected.is_some(),
        );
        let remaining = (primary == Primary::Play)
            .then_some(self.resume)
            .flatten()
            .and_then(|point| {
                crate::film_display::format_time_left(point.offset_secs, Some(point.duration_secs))
            });
        let label = remaining.as_ref().map_or_else(
            || primary.label().to_owned(),
            |time| format!("Continue, {time} left"),
        );
        // The bar carries how far in; the pill names the action, as the Home hero does.
        let mut button = Button::primary(
            "film-primary",
            if remaining.is_some() {
                "Continue"
            } else {
                primary.label()
            },
        )
        .aria_label(label.clone())
        .busy(primary == Primary::Loading)
        .disabled(!primary.enabled())
        .reduced_motion(view.reduced)
        .focus(&self.primary, view.keyboard)
        .tab_stop(view.active)
        .on_click(cx.listener(|film, _, window, cx| film.primary_action(false, window, cx)));
        if primary == Primary::Play {
            button = button.leading(Icon::Play);
        }
        if remaining.is_some()
            && let Some(point) = self.resume
        {
            button = button.progress((point.offset_secs / point.duration_secs) as f32);
        }
        let watchlist = Button::round(
            "film-watchlist",
            if self.saved { Icon::Check } else { Icon::Plus },
            if self.saved {
                "Remove from Watchlist"
            } else {
                "Add to Watchlist"
            },
        )
        .disabled(self.saving)
        .focus(&self.watchlist, view.keyboard)
        .tab_stop(view.active)
        .on_click(cx.listener(|film, _, window, cx| film.toggle_watchlist(window, cx)));
        let restart = remaining.is_some().then(|| {
            Button::glass("film-restart", "Start over")
                .focus(&self.restart, view.keyboard)
                .tab_stop(view.active)
                .on_click(cx.listener(|film, _, window, cx| film.primary_action(true, window, cx)))
        });
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(12.0))
            .child(button)
            .children(restart)
            .child(watchlist)
            .child(crate::a11y::status("film-primary-status", label))
    }
    /// Return to the previous route, or Home when Film opened the window.
    pub(super) fn back(&self, view: ViewSettings, cx: &mut Context<Self>) -> Button {
        Button::round("film-back", Icon::ArrowLeft, "Back")
            .compact()
            .focus(&self.back, view.keyboard)
            .tab_stop(view.active)
            .on_click(cx.listener(|film, _, window, cx| {
                let entry = film.entry;
                let _ = film.shell.update(cx, |shell, cx| {
                    shell.back(window, cx);
                    if shell.is_current(entry) {
                        shell.go(crate::router::Destination::Home, window, cx);
                    }
                });
            }))
    }
    pub(super) fn retry(
        &self,
        details: bool,
        view: ViewSettings,
        cx: &mut Context<Self>,
    ) -> Button {
        let label = if details {
            "Retry details"
        } else {
            "Retry sources"
        };
        Button::glass(label, label)
            .compact()
            .leading(Icon::Retry)
            .focus(
                if details {
                    &self.retry_details
                } else {
                    &self.retry_sources
                },
                view.keyboard,
            )
            .tab_stop(view.active)
            .on_click(cx.listener(move |film, _, _, cx| {
                if details {
                    film.load_details(cx);
                } else {
                    film.load_sources(cx);
                }
            }))
    }
}
