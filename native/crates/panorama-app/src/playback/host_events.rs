use super::*;

impl Runner {
    pub(super) async fn player_command(&mut self, command: Command) {
        if matches!(command, Command::Seek(value) | Command::SeekRelative(value) if !value.is_finite())
        {
            return;
        }
        let command = match command {
            Command::TogglePause => Command::SetPaused(!self.model.paused),
            Command::SeekRelative(delta) => Command::Seek((self.position + delta).max(0.0)),
            Command::Seek(seconds) => Command::Seek(seconds.max(0.0)),
            Command::SetVolume(volume) => Command::SetVolume(volume.min(100)),
            command => command,
        };
        if let Some(active) = &mut self.active {
            if let Err(error) = active.command(&command) {
                self.emit(HostEvent::Failed(error));
                return;
            }
            if matches!(command, Command::Seek(_)) {
                self.resume = None;
                self.awaiting_resume = None;
            }
        } else if let Command::Seek(seconds) = command {
            self.resume = Some(seconds);
            self.awaiting_resume = Some(seconds);
            if let Some(selection) = &mut self.selection {
                selection.start = StartMode::Resume(seconds);
            }
        }
        match command {
            Command::SetPaused(paused) => {
                self.emit(HostEvent::Mpv(PlayerEvent::Pause(paused)));
                self.sample(false).await;
            }
            Command::SetVolume(volume) => self.emit(HostEvent::Volume(volume)),
            Command::SetMuted(muted) => self.emit(HostEvent::Muted(muted)),
            Command::Seek(seconds) => {
                self.position = seconds;
                self.emit(HostEvent::Seeked(seconds));
                self.sample(true).await;
            }
            _ => {}
        }
        self.presence();
    }

    pub(super) async fn sample(&mut self, seek: bool) {
        let Some(selection) = &self.selection else {
            return;
        };
        if !self.begun || self.awaiting_resume.is_some() {
            return;
        }
        let sample = PlaybackSample {
            time_secs: self.model.time.unwrap_or(0.0),
            duration_secs: self.model.duration.unwrap_or(0.0),
            paused: self.model.paused,
        };
        self.progress
            .send(if seek {
                Progress::Seek(selection.film.id.clone(), sample)
            } else {
                Progress::Sample(selection.film.id.clone(), sample)
            })
            .await;
    }

    pub(super) async fn tick(&mut self) {
        let events = self
            .active
            .as_mut()
            .map(|active| active.events())
            .unwrap_or_default();
        for event in events {
            if let PlayerEvent::TimePos(Some(seconds)) = &event
                && seconds.is_finite()
                && *seconds >= 0.0
                && self.resume.is_none()
                && self
                    .awaiting_resume
                    .is_none_or(|target| (*seconds - target).abs() <= 3.0)
            {
                self.awaiting_resume = None;
                self.position = *seconds;
            }
            if let PlayerEvent::Duration(Some(duration)) = &event
                && duration.is_finite()
                && *duration > 0.0
                && let Some(offset) = self.resume.take()
                && let Some(active) = &mut self.active
            {
                let offset = if offset.is_finite() {
                    offset.max(0.0).min(*duration)
                } else {
                    0.0
                };
                self.position = offset;
                self.awaiting_resume = Some(offset);
                if let Err(error) = active.command(&Command::Seek(offset)) {
                    self.emit(HostEvent::Failed(error));
                }
            }
            let mut link = None;
            if let PlayerEvent::DemuxerCacheState(cache) = &event
                && let Some(active) = &self.active
            {
                link = active.cache(
                    cache,
                    self.origin.elapsed().as_millis() as u64,
                    self.model.duration,
                );
            }
            let time = matches!(event, PlayerEvent::TimePos(Some(_)));
            let pause = matches!(event, PlayerEvent::Pause(_));
            let was_ended = self.model.status == Status::Ended;
            let shutdown = matches!(event, PlayerEvent::Shutdown);
            self.emit(HostEvent::Mpv(event));
            if let Some(link) = link
                && self
                    .last_link
                    .is_none_or(|last| last.elapsed() >= self.link_interval)
            {
                self.last_link = Some(Instant::now());
                self.emit(HostEvent::Link(link));
            }
            let ended = !was_ended && self.model.status == Status::Ended;
            if !self.begun
                && self.model.status == Status::Ready
                && let Some(selection) = &self.selection
            {
                self.progress
                    .send(Progress::Begin(
                        Box::new(selection.clone()),
                        selection.start.seconds(),
                    ))
                    .await;
                self.begun = true;
            }
            if time || pause {
                self.sample(false).await;
            }
            if ended
                && self.begun
                && let Some(selection) = &self.selection
            {
                self.progress
                    .send(Progress::Finish(selection.film.id.clone()))
                    .await;
            }
            if shutdown && self.model.surface_visible() {
                self.emit(HostEvent::Failed(
                    crate::playback::model::PlaybackError::Unknown,
                ));
            }
        }
        self.presence();
    }

    /// Once a second: a transport failure, or else the torrent's health. The engine reports
    /// `Stalled` and `NoPeers` only together with an error, so an error replaces the reading.
    pub(super) fn poll_stats(&mut self) {
        if self.model.status == Status::NoPeers {
            return;
        }
        let Some(active) = self.active.as_ref() else {
            return;
        };
        let (error, stats) = (active.error(), active.torrent());
        if let Some(error) = error {
            if error == PlaybackError::NoPeers || !matches!(self.model.status, Status::Error(_)) {
                self.emit(HostEvent::Failed(error));
            }
            if error == PlaybackError::NoPeers {
                // mpv may still be playing from its buffer; keep sound off under the overlay.
                if let Some(active) = &mut self.active {
                    let _ = active.command(&Command::SetPaused(true));
                }
                self.emit(HostEvent::Mpv(PlayerEvent::Pause(true)));
            }
            return;
        }
        if let Some(stats) = stats
            && !matches!(self.model.status, Status::Error(_))
        {
            let since = self
                .torrent_since
                .map_or(0, |since| since.elapsed().as_millis());
            self.emit(HostEvent::Torrent(TorrentHealth {
                state: stats.state,
                peers_live: stats.peers_live,
                peers_connecting: stats.peers_connecting,
                download_bps: stats.download_bps,
                since_ms: since.try_into().unwrap_or(u64::MAX),
            }));
        }
    }

    pub(super) fn presence(&mut self) {
        if self
            .last_presence
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(1))
            && let Some(selection) = &self.selection
        {
            self.progress.presence(selection, &self.model);
            self.last_presence = Some(Instant::now());
        }
    }
}
