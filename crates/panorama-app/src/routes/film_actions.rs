use super::*;
use crate::{
    button::{Button, Icon},
    film_display::Primary,
};
use gpui::{Div, div, prelude::*, px};

impl Film {
    /// Start playback, or sign in first.
    pub(super) fn primary_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.account, Account::SignedIn(_)) {
            self.sign_in(window, cx);
            return;
        }
        if self.selected.is_none() {
            if !self.sources_loading {
                self.load_sources(true, cx);
            }
            return;
        }
        // Play does not wait for the subtitle files: a lookup still running follows the player.
        let late_subtitles = if self.subtitles.is_none() {
            self.subtitle_lookup.clone()
        } else {
            None
        };
        let subtitles = self.subtitles.clone().unwrap_or_default();
        if let (Some(source), Some(film)) = (self.selected, self.metadata.clone()) {
            let id = self.id.clone();
            let selection = crate::playback::selection::PlaybackSelection {
                video_id: id.clone(),
                film,
                groups: self.groups.clone(),
                source,
                start: self
                    .resume
                    .as_ref()
                    .map_or(crate::playback::selection::StartMode::Restart, |point| {
                        crate::playback::selection::StartMode::Resume(point.offset_secs)
                    }),
                meta_base: self.meta_base.clone(),
                subtitles,
                late_subtitles,
            };
            self.state
                .update(cx, |state, _| state.playback = Some(selection));
            let route = crate::router::Route::Player { id };
            let _ = self
                .shell
                .update(cx, |shell, cx| shell.navigate(route, window, cx));
        }
    }
    pub(super) fn actions(&mut self, view: ViewSettings, cx: &mut Context<Self>) -> Div {
        let primary = Primary::at(
            matches!(self.account, Account::SignedIn(_)),
            self.sources_loading,
            self.selected.is_some(),
        );
        let remaining = (primary == Primary::Play)
            .then_some(self.resume.clone())
            .flatten()
            .and_then(|point| {
                crate::film_display::format_time_left(point.offset_secs, Some(point.duration_secs))
            });
        let label = match (&remaining, primary) {
            (Some(time), _) => format!("Continue, {time} left"),
            (None, Primary::Unavailable) if self.sources_error => {
                "Sources could not be loaded. Retry sources".to_owned()
            }
            (None, Primary::Unavailable) => {
                "No playable source from your addons. Retry sources".to_owned()
            }
            (None, primary) => primary.label().to_owned(),
        };
        // Resuming, the pill reads play, how far in, then the time left, as the Home hero does.
        // With nothing to play the pill turns grey and retries, so no extra status row is needed.
        let text = remaining
            .clone()
            .unwrap_or_else(|| primary.label().to_owned());
        let mut button = if primary == Primary::Unavailable {
            Button::glass("film-primary", text).leading(Icon::Retry)
        } else {
            Button::primary("film-primary", text)
        }
        .aria_label(label.clone())
        .busy(primary == Primary::Loading)
        .disabled(!primary.enabled())
        .reduced_motion(view.reduced)
        .focus(&self.primary, view.keyboard)
        .tab_stop(view.active)
        .on_click(cx.listener(|film, _, window, cx| film.primary_action(window, cx)));
        if primary == Primary::Play {
            button = button.leading(Icon::Play);
        }
        if remaining.is_some()
            && let Some(point) = &self.resume
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
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(12.0))
            .child(button)
            .child(watchlist)
            .child(crate::a11y::status("film-primary-status", label))
    }
    pub(super) fn back(&self, view: ViewSettings, cx: &mut Context<Self>) -> Button {
        Button::round("film-back", Icon::ArrowLeft, "Back")
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
    pub(super) fn retry(&self, view: ViewSettings, cx: &mut Context<Self>) -> Button {
        Button::glass("Retry details", "Retry details")
            .compact()
            .leading(Icon::Retry)
            .focus(&self.retry_details, view.keyboard)
            .tab_stop(view.active)
            .on_click(cx.listener(|film, _, _, cx| film.load_details(cx)))
    }
}
