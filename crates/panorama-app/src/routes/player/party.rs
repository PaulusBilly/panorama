//! The player's side of a watch party: it reports itself, announces what the host plays,
//! tells a member whether it found the host's release, and routes the user's play, pause and
//! seek through the party.
use super::*;
use crate::{
    party::{member_state, release_matches, user_action},
    playback::model::Status,
};
use panorama_core::{
    addons::StreamState,
    party::{MemberState, PartyMedia, PartyRelease},
};
use std::time::Duration;

/// While playing, the party hears the position this often, so drift correction stays fine.
const REPORT_EVERY: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct PartyTap {
    reported: Option<(MemberState, Instant)>,
    /// The player was opening a file; the next ready frame is a reopen.
    opening: bool,
}

impl Player {
    /// After the model took `event`.
    pub(super) fn tap_party(&mut self, changed: Option<SourceRef>, cx: &mut Context<Self>) {
        let Some((host, steering, media)) = self.state.read(cx).party.as_ref().map(|party| {
            (
                party.view.is_host(),
                party.view.ended().is_none(),
                party.media().cloned(),
            )
        }) else {
            self.party = PartyTap::default();
            return;
        };
        if !steering {
            return;
        }
        let opened =
            self.party.opening && matches!(self.model.status, Status::Ready | Status::Buffering);
        self.party.opening = matches!(self.model.status, Status::Preparing(_))
            || (self.party.opening && !opened && !matches!(self.model.status, Status::Error(_)));
        if host {
            if changed.is_some() || (media.is_none() && self.model.has_reached_playable) {
                self.announce(cx);
            }
        } else {
            if let (Some(source), Some(media)) = (changed, &media) {
                let matched = self.plays_release(source, &media.release);
                // Everyone opens paused; the party's timeline starts them.
                self.send_direct(Command::SetPaused(true), cx);
                self.state.read(cx).party_release(matched);
            }
            if opened {
                self.state.read(cx).party_reopened();
            }
        }
        self.report(cx);
    }

    fn report(&mut self, cx: &mut Context<Self>) {
        let Some((state, position, duration)) = member_state(&self.model) else {
            return;
        };
        let now = Instant::now();
        let due = self.party.reported.is_none_or(|(last, at)| {
            last != state
                || (state == MemberState::Playing && now.duration_since(at) >= REPORT_EVERY)
        });
        if due {
            self.party.reported = Some((state, now));
            self.state.read(cx).party_report(state, position, duration);
        }
    }

    /// Host: tell the party what is playing, from where.
    fn announce(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.selection(cx) else {
            return;
        };
        let source = self.model.source;
        let release = [self.groups.as_slice(), selection.groups.as_slice()]
            .into_iter()
            .find_map(|groups| {
                let group = groups.get(source.group)?;
                let StreamState::Ready(streams) = &group.state else {
                    return None;
                };
                Some(PartyRelease::of(group, streams.get(source.stream)?))
            });
        let Some(release) = release else {
            return;
        };
        let media = PartyMedia {
            film_id: selection.film.id.clone(),
            kind: selection.film.kind,
            video_id: selection.video_id.clone(),
            title: self.model.title.chars().take(200).collect(),
            release,
        };
        let at = self.model.time.unwrap_or(selection.start.seconds());
        self.state
            .update(cx, |state, _| state.party_media(media, at));
    }

    fn plays_release(&self, source: SourceRef, release: &PartyRelease) -> bool {
        self.groups.get(source.group).is_some_and(|group| {
            let StreamState::Ready(streams) = &group.state else {
                return false;
            };
            streams
                .get(source.stream)
                .is_some_and(|stream| release_matches(release, group, stream))
        })
    }

    /// The user's play, pause or seek, when a party decides it. `true` when the party took it.
    pub(super) fn route_through_party(&self, command: &Command, cx: &gpui::App) -> bool {
        let state = self.state.read(cx);
        if !state
            .party
            .as_ref()
            .is_some_and(|party| party.steers_playback())
        {
            return false;
        }
        let Some(action) = user_action(command, &self.model) else {
            return false;
        };
        state.party_action(action);
        true
    }

    /// Straight to the playback host, never through the party.
    fn send_direct(&self, command: Command, cx: &gpui::App) {
        if let Some(host) = &self.state.read(cx).playback_host {
            host.send(command);
        }
    }

    /// A party member never rolls into the next episode on its own.
    pub(super) fn up_next_allowed(&self, cx: &gpui::App) -> bool {
        crate::party::up_next_allowed(self.state.read(cx).party.as_ref().map(|party| &party.view))
    }
}
