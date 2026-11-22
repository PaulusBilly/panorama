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
        if let Some(selection) = self.selection() {
            self.open_player(selection, window, cx);
        }
    }
    /// Play what Play would, from its beginning instead of where it was left.
    fn start_over(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut selection) = self.selection() {
            selection.start = crate::playback::selection::StartMode::Restart;
            self.open_player(selection, window, cx);
        }
    }
    fn open_player(
        &mut self,
        selection: crate::playback::selection::PlaybackSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let route = crate::router::Route::Player {
            id: self.id.clone(),
        };
        self.state
            .update(cx, |state, _| state.playback = Some(selection));
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.navigate(route, window, cx));
    }
    /// What Play would play now: the selected source of the film or the target episode.
    fn selection(&self) -> Option<crate::playback::selection::PlaybackSelection> {
        // Play does not wait for the subtitle files: a lookup still running follows the player.
        let late_subtitles = if self.subtitles.is_none() {
            self.subtitle_lookup.clone()
        } else {
            None
        };
        let mut film = self.metadata.clone()?;
        film.episodes = (*self.episodes).clone();
        Some(crate::playback::selection::PlaybackSelection {
            video_id: self.target.clone()?,
            film,
            groups: self.groups.clone(),
            source: self.selected?,
            start: self
                .resume_here()
                .map_or(crate::playback::selection::StartMode::Restart, |point| {
                    crate::playback::selection::StartMode::Resume(point.offset_secs)
                }),
            meta_base: self.meta_base.clone(),
            subtitles: self.subtitles.clone().unwrap_or_default(),
            late_subtitles,
        })
    }
    /// Host a watch party and open its lobby; the film plays once the host starts it there.
    fn watch_together(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self
            .selection()
            .filter(|_| matches!(self.account, Account::SignedIn(_)));
        let Some(selection) = selection else {
            self.primary_action(window, cx);
            return;
        };
        self.state
            .update(cx, |state, cx| state.start_party(None, cx));
        let _ = self.shell.update(cx, |shell, cx| {
            shell.open_party_lobby_as_host(selection, window, cx)
        });
    }
    /// The resume point, when it belongs to what Play plays.
    fn resume_here(&self) -> Option<&panorama_core::stremio::ResumePoint> {
        self.resume
            .as_ref()
            .filter(|point| point.video_id.is_none() || point.video_id == self.target)
    }
    pub(super) fn actions(&mut self, view: ViewSettings, cx: &mut Context<Self>) -> Div {
        let primary = Primary::at(
            matches!(self.account, Account::SignedIn(_)),
            self.sources_loading,
            self.selected.is_some(),
        );
        let episode = self.target_episode().map(super::episodes::code);
        let remaining = (primary == Primary::Play)
            .then(|| self.resume_here().cloned())
            .flatten()
            .and_then(|point| {
                crate::film_display::format_time_left(point.offset_secs, Some(point.duration_secs))
            });
        let label = match (&remaining, primary) {
            (Some(time), _) => match &episode {
                Some(code) => format!("Continue {code}, {time} left"),
                None => format!("Continue, {time} left"),
            },
            (None, Primary::Unavailable) if self.sources_error => {
                "Sources could not be loaded. Retry sources".to_owned()
            }
            (None, Primary::Unavailable) => {
                "No playable source from your addons. Retry sources".to_owned()
            }
            (None, Primary::Play) if episode.is_some() => {
                format!("Play {}", episode.as_deref().unwrap_or_default())
            }
            (None, primary) => primary.label().to_owned(),
        };
        // Resuming, the pill reads play, how far in, then the time left, as the Home hero does.
        // With nothing to play the pill turns grey and retries, so no extra status row is needed.
        let text = match (&remaining, &episode, primary) {
            (Some(time), Some(code), _) => format!("{code} · {time}"),
            (Some(time), None, _) => time.clone(),
            (None, Some(code), Primary::Play) => format!("Play {code}"),
            _ => primary.label().to_owned(),
        };
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
            && let Some(point) = self.resume_here()
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
        let together = Button::round("film-watch-together", Icon::Users, "Watch together")
            .disabled(primary != Primary::Play)
            .focus(&self.watch_together, view.keyboard)
            .tab_stop(view.active)
            .on_click(cx.listener(|film, _, window, cx| film.watch_together(window, cx)));
        // Only beside a Continue: with nothing to resume, Play already starts at the beginning.
        let start_over = remaining.is_some().then(|| {
            let name = match &episode {
                Some(code) => format!("Start {code} over"),
                None => "Start over".to_owned(),
            };
            Button::round("film-start-over", Icon::StartOver, name)
                .focus(&self.start_over, view.keyboard)
                .tab_stop(view.active)
                .on_click(cx.listener(|film, _, window, cx| film.start_over(window, cx)))
        });
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(12.0))
            .child(button)
            .children(start_over)
            .child(watchlist)
            .child(together)
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
