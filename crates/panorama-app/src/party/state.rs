//! The party on `AppState`: start, join, leave, settings, and what the player feeds in.
use super::{
    PartyView, Role, default_name, display_name,
    service::{self, Event, Input},
};
use crate::{
    app_state::{Account, AppState},
    playback::{host::Command, launch::Pick},
};
use futures::StreamExt;
use gpui::{Context, Task};
use panorama_core::{
    addons::{FilmDetails, Kind, ResourceEvent},
    party::{
        Action, HostSettings, MemberState, PartyMedia,
        transport::{InviteError, parse_invite},
    },
    store::Key,
};
use tokio::sync::mpsc::UnboundedSender;

const NAME: &str = "party.name";

/// The party this window is in, from start until the user leaves. An ended party stays, so
/// its reason shows, until left.
pub struct Party {
    pub view: PartyView,
    inputs: UnboundedSender<Input>,
    /// Host: the media last announced. Member: the media last loaded.
    media: Option<PartyMedia>,
    _events: Task<()>,
}

impl Party {
    /// A party with a fixed view and no network, for `--party-preview` captures.
    pub fn preview(view: PartyView) -> Self {
        let (inputs, _) = tokio::sync::mpsc::unbounded_channel();
        Self {
            view,
            inputs,
            media: None,
            _events: Task::ready(()),
        }
    }

    pub fn media(&self) -> Option<&PartyMedia> {
        self.media.as_ref()
    }

    /// Whether the user's play, pause and seek go through the party instead of the player.
    pub fn steers_playback(&self) -> bool {
        self.view.ended().is_none() && (!self.view.is_host() || self.media.is_some())
    }

    fn send(&self, input: Input) {
        let _ = self.inputs.send(input);
    }
}

/// A film to open by id once the account and its addons are ready: for a member, the host's
/// media; for debugging, `--party-play`.
pub struct PendingPlay {
    kind: Kind,
    film_id: String,
    video_id: String,
    pick: Pick,
    stage: Stage,
}

enum Stage {
    Waiting,
    Fetching { _task: Task<()> },
    Ready(Box<FilmDetails>),
}

impl AppState {
    /// The name this install shows in a party.
    pub fn party_name(&self) -> String {
        let stored = (|| {
            let key = Key::pref(NAME).ok()?;
            let bytes = self.services.as_ref()?.store.as_ref()?.get(&key).ok()??;
            display_name(&String::from_utf8(bytes).ok()?)
        })();
        stored.unwrap_or_else(default_name)
    }

    /// Keep a display name for later parties. `false` when it is empty once trimmed.
    pub fn set_party_name(&mut self, name: &str) -> bool {
        let Some(name) = display_name(name) else {
            return false;
        };
        if let (Some(store), Ok(key)) = (
            self.services.as_ref().and_then(|s| s.store.as_ref()),
            Key::pref(NAME),
        ) {
            let _ = store.set(&key, name.as_bytes());
        }
        true
    }

    /// Host a party. Whatever plays next, or plays now, is what the party watches. Any party
    /// this window was in is left first.
    pub fn start_party(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let name = name
            .and_then(|name| display_name(&name))
            .unwrap_or_else(|| self.party_name());
        let settings = HostSettings::default();
        let spawned = service::host(&services.runtime, services.store.clone(), name, settings);
        let role = Role::Host {
            invite: None,
            settings,
        };
        self.attach_party(spawned, role, cx);
    }

    /// Join with an invite link. A link that is not one is refused here; anything that goes
    /// wrong later shows in the party's view.
    pub fn join_party(
        &mut self,
        invite: &str,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), InviteError> {
        let invite = parse_invite(invite)?;
        let Some(services) = self.services.clone() else {
            return Ok(());
        };
        let name = name
            .and_then(|name| display_name(&name))
            .unwrap_or_else(|| self.party_name());
        let spawned = service::member(&services.runtime, services.store.clone(), name, invite);
        self.attach_party(spawned, Role::Member, cx);
        Ok(())
    }

    /// Leave, or as the host, end the party for everyone. Playback carries on.
    pub fn leave_party(&mut self, cx: &mut Context<Self>) {
        let Some(party) = self.party.take() else {
            return;
        };
        if !party.view.is_host()
            && let Some(host) = &self.playback_host
        {
            host.send(Command::SetSpeed(1.0));
        }
        if self
            .pending_play
            .as_ref()
            .is_some_and(|play| matches!(play.pick, Pick::Party(_)))
        {
            self.pending_play = None;
        }
        cx.notify();
    }

    /// Host only.
    pub fn set_party_settings(&mut self, settings: HostSettings, cx: &mut Context<Self>) {
        if let Some(party) = self.party.as_ref().filter(|party| party.view.is_host()) {
            party.send(Input::Settings(settings));
            cx.notify();
        }
    }

    /// The user's own play, pause or seek, while the party steers playback.
    pub fn party_action(&self, action: Action) {
        if let Some(party) = &self.party {
            party.send(Input::Local(action));
        }
    }

    /// The local player's state, on change and while playing.
    pub fn party_report(&self, state: MemberState, position: f64, duration: Option<f64>) {
        if let Some(party) = &self.party {
            party.send(Input::Report {
                state,
                position,
                duration,
            });
        }
    }

    /// Host: the local player opened `media` at `at` seconds. Announcing the same media again
    /// does nothing.
    pub fn party_media(&mut self, media: PartyMedia, at: f64) {
        if let Some(party) = self
            .party
            .as_mut()
            .filter(|party| party.view.is_host() && party.media.as_ref() != Some(&media))
        {
            party.media = Some(media.clone());
            party.send(Input::Media { media, at });
        }
    }

    /// Member: whether the source now playing is the host's release.
    pub fn party_release(&self, matched: bool) {
        if let Some(party) = &self.party {
            party.send(Input::Release { matched });
        }
    }

    /// Member: the player opened a new file, so the party's timeline applies again.
    pub fn party_reopened(&self) {
        if let Some(party) = &self.party {
            party.send(Input::Reopened);
        }
    }

    /// Open a film by id once the account is ready, starting with `pick`'s source.
    pub fn play_by_id(&mut self, kind: Kind, film_id: String, video_id: String, pick: Pick) {
        self.pending_play = Some(PendingPlay {
            kind,
            film_id,
            video_id,
            pick,
            stage: Stage::Waiting,
        });
    }

    /// Moves a pending play along; hands back the film, video and pick once its details are
    /// in, for the shell to launch and navigate.
    pub fn take_ready_play(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(FilmDetails, String, Pick)> {
        let ready = matches!(self.account, Account::SignedIn(_)) && !self.installed.is_empty();
        let play = self.pending_play.as_mut()?;
        match &play.stage {
            Stage::Waiting if ready => {
                let services = self.services.clone()?;
                let (job, mut receiver) = services.details(play.kind, play.film_id.clone());
                let film_id = play.film_id.clone();
                play.stage = Stage::Fetching {
                    _task: cx.spawn(async move |state, cx| {
                        let _job = job;
                        let found = receiver.next().await.and_then(|event| match event {
                            ResourceEvent::CacheHit(details) | ResourceEvent::Fresh(details) => {
                                Some(details)
                            }
                            ResourceEvent::Failed(_) => None,
                        });
                        if let Some(details) = found {
                            let _ = state.update(cx, |state, cx| {
                                if let Some(play) = state
                                    .pending_play
                                    .as_mut()
                                    .filter(|play| play.film_id == film_id)
                                {
                                    play.stage = Stage::Ready(Box::new(details));
                                    cx.notify();
                                }
                            });
                            return;
                        }
                        super::trace(|| "pending play details failed".into());
                        let _ = state.update(cx, |state, _| state.pending_play = None);
                    }),
                };
                None
            }
            Stage::Ready(_) => {
                let play = self.pending_play.take()?;
                let Stage::Ready(film) = play.stage else {
                    return None;
                };
                Some((*film, play.video_id, play.pick))
            }
            Stage::Waiting | Stage::Fetching { .. } => None,
        }
    }

    fn attach_party(&mut self, spawned: service::Spawned, role: Role, cx: &mut Context<Self>) {
        self.leave_party(cx);
        let mut events = spawned.events;
        let task = cx.spawn(async move |state, cx| {
            while let Some(event) = events.next().await {
                if state
                    .update(cx, |state, cx| state.party_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        self.party = Some(Party {
            view: PartyView::new(role, super::Link::Starting),
            inputs: spawned.inputs,
            media: None,
            _events: task,
        });
        cx.notify();
    }

    fn party_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let Some(party) = self.party.as_mut() else {
            return;
        };
        match event {
            Event::View(view) => party.view = view,
            Event::Player(command) => {
                if let Some(host) = &self.playback_host {
                    host.send(command);
                }
            }
            Event::Load(media) => {
                party.media = Some(media.clone());
                self.play_by_id(
                    media.kind,
                    media.film_id,
                    media.video_id,
                    Pick::Party(media.release),
                );
            }
        }
        cx.notify();
    }
}
