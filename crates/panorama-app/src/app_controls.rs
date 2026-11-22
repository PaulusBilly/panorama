use super::*;

impl AppShell {
    /// Open a fresh sign-in form above all route chrome.
    pub fn open_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu.set_open(false);
        gpui_component::Theme::update(cx, |theme| theme.colors.caret = self.theme.ink.into());
        if self.login.is_none() {
            let shell = cx.weak_entity();
            let state = self.state.clone();
            self.login = Some(cx.new(|cx| LoginDialog::new(shell, state, window, cx)));
        }
        cx.notify();
    }
    /// Quit into the downloaded update; Velopack's updater reopens Panorama once it's applied.
    /// If the updater can't start, Panorama stays open and still tries again when it quits.
    pub fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        let Some(ready) = self.state.read(cx).update.clone() else {
            return;
        };
        if crate::updater::apply_after_exit(&ready, true) {
            self.state
                .update(cx, |state, _| state.update_restarting = true);
            cx.quit();
        }
    }
    pub fn finish_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.login = None;
        self.sidebar.account.focus(window, cx);
        cx.notify();
    }

    /// The host's lobby for the party just started, before `selection` plays.
    pub fn open_party_lobby_as_host(
        &mut self,
        selection: crate::playback::selection::PlaybackSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let side = crate::party_lobby::Side::Host(Box::new(selection));
        self.open_party_lobby(side, None, window, cx);
    }

    /// The lobby of the party this window just joined. The host's film opens under it and
    /// shows once the host starts the party.
    pub fn open_party_lobby_as_member(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_party_lobby(crate::party_lobby::Side::Member, None, window, cx);
    }

    /// `--open-party-lobby`: a made-up party over the current route, for captures.
    pub(crate) fn open_party_lobby_preview(
        &mut self,
        host: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = match self.history.current() {
            Route::Film { id, .. } => id.clone(),
            _ => String::new(),
        };
        let Some(film) =
            crate::fixtures::details(&id).or_else(|| crate::fixtures::films().into_iter().next())
        else {
            return;
        };
        let view = crate::party_lobby::preview(host);
        self.state.update(cx, |state, _| {
            state.party = Some(crate::party::Party::preview(view));
        });
        let side = if host {
            crate::party_lobby::Side::Host(Box::new(
                crate::playback::selection::PlaybackSelection {
                    video_id: film.id.clone(),
                    film: film.clone(),
                    groups: crate::fixtures::player_groups(),
                    source: crate::playback::selection::SourceRef {
                        group: 0,
                        stream: 4,
                    },
                    start: crate::playback::selection::StartMode::Restart,
                    meta_base: None,
                    subtitles: Vec::new(),
                    late_subtitles: None,
                },
            ))
        } else {
            crate::party_lobby::Side::Member
        };
        self.open_party_lobby(side, Some(film), window, cx);
    }

    fn open_party_lobby(
        &mut self,
        side: crate::party_lobby::Side,
        film: Option<panorama_core::addons::FilmDetails>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu.set_open(false);
        let shell = cx.weak_entity();
        let state = self.state.clone();
        self.lobby =
            Some(cx.new(|cx| {
                crate::party_lobby::PartyLobby::new(shell, state, side, film, window, cx)
            }));
        cx.notify();
    }

    /// After leaving from the lobby: a friend's player, opened under it, closes too.
    pub(crate) fn leave_party_route(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Route::Player { id } = self.history.current().clone() {
            if let Some(host) = &self.state.read(cx).playback_host {
                host.send(crate::playback::host::Command::Stop);
            }
            self.back_to_film(id, window, cx);
        }
    }

    pub(crate) fn finish_party_lobby(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.lobby = None;
        // The host closed the film while this lobby was still fading out.
        if self.state.read(cx).party_stopped() {
            return self.open_party_lobby_as_member(window, cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }
}
