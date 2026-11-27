//! The player's side of a watch party: it reports itself, announces what the host plays,
//! tells a member whether it found the host's release, and routes the user's play, pause and
//! seek through the party.
use super::*;
use crate::{
    party::{
        PartySync,
        follow::{self, Followed},
        member_state, release_matches, user_action,
    },
    playback::model::Status,
};
use panorama_core::{
    addons::StreamState,
    party::{MemberState, PartyMedia, PartyRelease, TrackChoice},
};
use std::time::Duration;

/// While playing, the party hears the position this often, so drift correction stays fine.
const REPORT_EVERY: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct PartyTap {
    reported: Option<(MemberState, f64, Instant)>,
    /// The player was opening a file; the next ready frame is a reopen.
    opening: bool,
    /// Host: the tracks the party last heard.
    announced: Option<TrackChoice>,
    /// Host: the source the host picked by hand, which the party follows. A fallback it
    /// did not pick stays the host's own.
    pub(super) picked: Option<SourceRef>,
    /// Member: the host's choice being followed.
    pub(super) followed: Option<Followed>,
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
            self.announce_tracks(cx);
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
            let failed = matches!(self.model.status, Status::Error(_) | Status::NoPeers);
            self.state
                .update(cx, |state, cx| state.set_party_source_failed(failed, cx));
            self.follow_host_tracks(cx);
        }
        self.report(cx);
    }

    /// Host: tell the party which audio and subtitle tracks are on, whenever they change. The
    /// player's track list is the source: mpv sends it again each time a selection moves.
    fn announce_tracks(&mut self, cx: &mut Context<Self>) {
        if self
            .state
            .read(cx)
            .party
            .as_ref()
            .is_none_or(|party| party.media().is_none())
        {
            return;
        }
        let Some(choice) = follow::choice_of(&self.model.audio, &self.model.subtitles) else {
            return;
        };
        let choice = choice.bounded();
        if self.party.announced.as_ref() == Some(&choice) {
            return;
        }
        self.party.announced = Some(choice.clone());
        self.state.read(cx).party_tracks(choice);
    }

    /// Member: put the audio and subtitles on the host's choice, for the kinds sync follows.
    /// A host track nothing here matches leaves the member's own pick, and is tried again when
    /// the track list grows, because an addon's subtitles arrive after the file's.
    pub(super) fn follow_host_tracks(&mut self, cx: &mut Context<Self>) {
        let Some((choice, sync)) = self.host_tracks(cx) else {
            self.party.followed = None;
            return;
        };
        if !self.ui.loaded {
            return;
        }
        let mut followed = match self.party.followed.take() {
            Some(followed) if followed.is(&choice, sync) => followed,
            _ => Followed::new(choice, sync),
        };
        let plan = followed.step(&self.model.audio, &self.model.subtitles);
        if let Some(id) = plan.audio {
            self.choose_audio(id, cx);
        }
        if let Some(id) = plan.subtitle {
            self.follow_subtitle(id, cx);
        }
        self.party.followed = Some(followed);
    }

    /// Member: the host's announced tracks and this install's sync setting.
    fn host_tracks(&self, cx: &Context<Self>) -> Option<(TrackChoice, PartySync)> {
        let party = self.state.read(cx).party.as_ref()?;
        if party.view.is_host() || party.view.ended().is_some() {
            return None;
        }
        Some((party.host_tracks()?.clone(), party.view.sync))
    }

    /// Member: the host's subtitle choice governs, so the viewer's remembered language must
    /// not turn a track on. Without this, a host with subtitles off settles the follow before
    /// an addon's file arrives, and the remembered language then switches that file on.
    pub(super) fn follows_host_subtitles(&self, cx: &Context<Self>) -> bool {
        self.host_tracks(cx).is_some_and(|(_, sync)| sync.subtitles)
    }

    fn report(&mut self, cx: &mut Context<Self>) {
        let Some((state, position, duration)) = member_state(&self.model) else {
            return;
        };
        let now = Instant::now();
        let due = self.party.reported.is_none_or(|(last, at_position, at)| {
            last != state
                || (state == MemberState::Playing && now.duration_since(at) >= REPORT_EVERY)
                // A seek can land without the state changing; the party waits on where.
                || (state != MemberState::Playing && (position - at_position).abs() > 1.0)
        });
        if due {
            self.party.reported = Some((state, position, now));
            self.state.read(cx).party_report(state, position, duration);
        }
    }

    /// Host: tell the party what is playing, from where. A fallback to another source of the
    /// same film or episode stays the host's own: members keep the copies they are playing.
    fn announce(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.selection(cx) else {
            return;
        };
        let announced = self
            .state
            .read(cx)
            .party
            .as_ref()
            .and_then(|party| party.media());
        let picked = self.party.picked == Some(self.model.source);
        if picked {
            self.party.picked = None;
        }
        if !picked
            && announced.is_some_and(|media| {
                media.film_id == selection.film.id && media.video_id == selection.video_id
            })
        {
            return;
        }
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
            .update(cx, |state, _| state.party_media_playing(media, at));
        self.party.announced = None;
        // The party forgets everyone's state on a load, so the next report must go out.
        self.party.reported = None;
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
