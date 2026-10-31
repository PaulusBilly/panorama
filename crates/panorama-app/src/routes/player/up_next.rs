//! Rolling into the next episode: offered near the end, then counted down once it has ended.
use super::*;
use crate::{
    button::Button,
    playback::model::Status,
    theme::{Theme, Typography},
};
use gpui::{Div, div, prelude::*, px, relative};
use panorama_core::addons::Episode;
use std::time::Duration;

/// How long before the end the next episode is offered.
const OFFER_SECS: f64 = 30.0;
/// How long the countdown runs after the episode ends.
const COUNTDOWN: Duration = Duration::from_secs(10);
/// Shorter videos are not offered early, so a brief episode is not covered for its whole length.
const OFFER_FROM_SECS: f64 = 120.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum UpNext {
    #[default]
    Hidden,
    Offered,
    Counting(Instant),
    /// Cancelled, until the viewer seeks back out of the closing stretch.
    Dismissed,
}

impl UpNext {
    /// `left` is the time remaining of a video `length` long.
    fn advance(self, left: Option<f64>, length: Option<f64>, ended: bool, now: Instant) -> Self {
        let closing = left.is_some_and(|left| left <= OFFER_SECS)
            && length.is_some_and(|length| length >= OFFER_FROM_SECS);
        match self {
            Self::Dismissed if ended || closing => self,
            Self::Counting(_) if ended => self,
            _ if ended => Self::Counting(now),
            _ if closing => Self::Offered,
            _ => Self::Hidden,
        }
    }

    fn seconds_left(self, now: Instant) -> Option<u64> {
        match self {
            Self::Counting(since) => {
                let left = COUNTDOWN.saturating_sub(now.saturating_duration_since(since));
                Some(left.as_secs_f32().ceil() as u64)
            }
            _ => None,
        }
    }
}

impl Player {
    /// Keep the card in step with the playhead; the countdown runs out into the next episode.
    pub(super) fn advance_up_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.next.is_none() {
            return;
        }
        let now = Instant::now();
        let (time, duration) = (self.model.time, self.model.duration);
        let left = time.zip(duration).map(|(time, duration)| duration - time);
        let ended = self.model.status == Status::Ended;
        self.up_next = self.up_next.advance(left, duration, ended, now);
        if let UpNext::Counting(_) = self.up_next {
            if self.up_next.seconds_left(now) == Some(0) {
                cx.on_next_frame(window, |player, _, cx| player.play_next(cx));
            } else {
                cx.on_next_frame(window, |_, _, cx| cx.notify());
                window.request_animation_frame();
            }
        }
    }

    pub(super) fn counting_down(&self) -> bool {
        matches!(self.up_next, UpNext::Counting(_))
    }

    pub(super) fn play_next(&mut self, cx: &mut Context<Self>) {
        let Some(next) = self.next.take() else {
            return;
        };
        let Some(selection) = self.selection(cx) else {
            return;
        };
        let group = selection
            .selected()
            .and_then(|source| source.stream.behavior_hints.binge_group.clone());
        self.unmount(cx);
        self.up_next = UpNext::Hidden;
        self.state.update(cx, |state, cx| {
            state.launch(selection.film, next.id, None, group, cx)
        });
        self.model.apply(HostEvent::Resolving);
        cx.notify();
    }

    pub(super) fn dismiss_up_next(&mut self, cx: &mut Context<Self>) {
        self.up_next = UpNext::Dismissed;
        cx.notify();
    }

    pub(super) fn up_next_card(&self, theme: Theme, cx: &mut Context<Self>) -> Option<Div> {
        let shown = matches!(self.up_next, UpNext::Offered | UpNext::Counting(_));
        let next = self.next.as_ref().filter(|_| shown)?;
        let heading = match self.up_next.seconds_left(Instant::now()) {
            Some(seconds) => format!("Next episode in {seconds}"),
            None => "Next episode".to_owned(),
        };
        Some(
            div()
                .absolute()
                .right(px(40.0))
                .bottom(px(120.0))
                .w(px(320.0))
                .p(px(20.0))
                .bg(theme.surface)
                .occlude()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("player-up-next")
                        .role(gpui::Role::Status)
                        .label()
                        .text_color(theme.ink_muted)
                        .child(heading),
                )
                .child(
                    div()
                        .body()
                        .mt(px(6.0))
                        .text_color(theme.ink)
                        .line_height(relative(1.3))
                        .line_clamp(2)
                        .child(episode_line(next)),
                )
                .child(
                    div()
                        .mt(px(16.0))
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::primary("up-next-play", "Play now")
                                .compact()
                                .on_click(cx.listener(|player, _, _, cx| player.play_next(cx))),
                        )
                        .child(
                            Button::glass("up-next-cancel", "Cancel")
                                .compact()
                                .on_click(
                                    cx.listener(|player, _, _, cx| player.dismiss_up_next(cx)),
                                ),
                        ),
                ),
        )
    }
}

/// How the player names an episode.
pub(crate) fn episode_line(episode: &Episode) -> String {
    format!(
        "S{} E{} · {}",
        episode.season, episode.number, episode.title
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: Option<f64> = Some(3000.0);

    #[test]
    fn the_card_follows_the_closing_stretch_and_the_end() {
        let now = Instant::now();
        let at = |state: UpNext, left| state.advance(Some(left), LONG, false, now);
        assert_eq!(at(UpNext::Hidden, 60.0), UpNext::Hidden);
        assert_eq!(at(UpNext::Hidden, 30.0), UpNext::Offered);
        assert_eq!(at(UpNext::Offered, 60.0), UpNext::Hidden, "seeked back");
        let counting = UpNext::Offered.advance(Some(0.0), LONG, true, now);
        assert_eq!(counting, UpNext::Counting(now));
        assert_eq!(
            counting.advance(Some(0.0), LONG, true, now + COUNTDOWN),
            counting,
            "the countdown keeps its start"
        );
        assert_eq!(at(counting, 900.0), UpNext::Hidden, "replayed");
    }

    #[test]
    fn a_cancel_holds_until_the_viewer_seeks_out_of_the_end() {
        let now = Instant::now();
        let dismissed = UpNext::Dismissed;
        assert_eq!(dismissed.advance(Some(0.0), LONG, true, now), dismissed);
        assert_eq!(dismissed.advance(Some(10.0), LONG, false, now), dismissed);
        assert_eq!(
            dismissed.advance(Some(600.0), LONG, false, now),
            UpNext::Hidden
        );
    }

    #[test]
    fn a_short_video_is_not_offered_early() {
        let now = Instant::now();
        let brief = UpNext::Hidden.advance(Some(20.0), Some(60.0), false, now);
        assert_eq!(brief, UpNext::Hidden);
    }

    #[test]
    fn the_countdown_reads_whole_seconds_down_to_zero() {
        let now = Instant::now();
        let counting = UpNext::Counting(now);
        assert_eq!(counting.seconds_left(now), Some(10));
        assert_eq!(
            counting.seconds_left(now + Duration::from_millis(9_500)),
            Some(1)
        );
        assert_eq!(counting.seconds_left(now + COUNTDOWN), Some(0));
        assert_eq!(UpNext::Offered.seconds_left(now), None);
    }
}
