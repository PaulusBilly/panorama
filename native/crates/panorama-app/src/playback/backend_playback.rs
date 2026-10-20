use super::*;

pub(super) enum Transport {
    File,
    Http(SessionHandle),
    Torrent(TorrentStream),
}

pub(super) struct NativePlayback {
    pub(super) library: MpvLibrary,
    /// Directory holding the Aspekta faces for libass; `None` falls back to system fonts.
    pub(super) fonts: Option<PathBuf>,
    pub(super) url: String,
    pub(super) transport: Transport,
    pub(super) player: Option<Player>,
    pub(super) events: Option<mpsc::Receiver<PlayerEvent>>,
    pub(super) cancel: CancellationToken,
    pub(super) shutdown: bool,
    pub(super) diagnostics: bool,
    pub(super) pending_seek: Option<f64>,
    pub(super) reported_time: bool,
    pub(super) close_started: Option<tokio::time::Instant>,
}

impl Playback for NativePlayback {
    fn load(
        &mut self,
        window: Option<RawWindow>,
        volume: u8,
        muted: bool,
        paused: bool,
    ) -> Result<(), PlaybackError> {
        let mut player = self
            .library
            .create(PlayerOptions {
                wid: window,
                extra: init_options(self.fonts.as_deref()),
            })
            .map_err(PlaybackError::from)?;
        self.events = Some(player.events().map_err(PlaybackError::from)?);
        player
            .set_volume(f64::from(volume))
            .map_err(PlaybackError::from)?;
        player
            .set_property("mute", muted)
            .map_err(PlaybackError::from)?;
        player.set_pause(paused).map_err(PlaybackError::from)?;
        player.load(&self.url).map_err(PlaybackError::from)?;
        self.player = Some(player);
        Ok(())
    }

    fn command(&mut self, command: &Command) -> Result<(), PlaybackError> {
        let Some(player) = &self.player else {
            return Ok(());
        };
        if let Command::Seek(seconds) = command {
            self.pending_seek = Some(*seconds);
        }
        match command {
            Command::SetPaused(paused) => player.set_pause(*paused),
            Command::TogglePause => player.toggle_pause(),
            Command::Seek(seconds) => player.seek(*seconds, SeekMode::Absolute),
            Command::SeekRelative(seconds) => player.seek(*seconds, SeekMode::Relative),
            Command::SetVolume(volume) => player.set_volume(f64::from((*volume).min(100))),
            Command::SetMuted(muted) => player.set_property("mute", *muted),
            Command::SelectAudio(id) => player.set_audio_track(Some(*id)),
            Command::SelectSubtitle(id) => player.set_subtitle_track(*id),
            Command::Subtitles(setup) => player
                .set_property("sub-visibility", setup.mpv_visible)
                .and_then(|()| player.set_subtitle_style(setup.style.clone())),
            Command::Output(properties) => properties
                .iter()
                .try_for_each(|(name, value)| player.set_property(name, value.as_str())),
            _ => Ok(()),
        }
        .map_err(PlaybackError::from)
    }

    fn events(&mut self) -> Vec<PlayerEvent> {
        let events: Vec<_> = self
            .events
            .as_ref()
            .map(|events| events.try_iter().collect())
            .unwrap_or_default();
        for event in &events {
            if *event == PlayerEvent::Shutdown {
                self.shutdown = true;
            }
            if !self.diagnostics {
                continue;
            }
            if let PlayerEvent::TimePos(Some(seconds)) = event
                && seconds.is_finite()
                && *seconds >= 0.0
                && !self.reported_time
            {
                self.reported_time = true;
                crate::debug::log("player file ready".into());
            }
            match event {
                PlayerEvent::HwdecCurrent(value) => {
                    let value = value
                        .as_deref()
                        .filter(|value| {
                            matches!(
                                *value,
                                "d3d11va"
                                    | "d3d11va-copy"
                                    | "dxva2"
                                    | "dxva2-copy"
                                    | "nvdec"
                                    | "nvdec-copy"
                                    | "auto"
                                    | "no"
                            )
                        })
                        .unwrap_or("none");
                    crate::debug::log(format!("player hwdec-current={value}"));
                }
                PlayerEvent::TimePos(Some(time))
                    if self
                        .pending_seek
                        .is_some_and(|target| (*time - target).abs() < 3.0) =>
                {
                    self.pending_seek = None;
                    crate::debug::log("player seek confirmed".into());
                }
                PlayerEvent::Error(error) => crate::debug::log(format!("player error={error:?}")),
                _ => {}
            }
        }
        events
    }

    fn cache(&self, cache: &CacheState, now_ms: u64, duration: Option<f64>) -> Option<LinkHealth> {
        let Transport::Http(session) = &self.transport else {
            return None;
        };
        let stats = session.stats();
        let source_mbps = stats
            .size_bytes
            .zip(duration)
            .filter(|(_, duration)| *duration > 0.0)
            .map(|(bytes, duration)| bytes as f64 * 8.0 / duration / 1_000_000.0);
        let download_mbps = stats.download_mbps.or(cache
            .input_bytes_per_second
            .map(|rate| rate * 8.0 / 1_000_000.0));
        session.set_read_ahead(BufferingSample {
            now_ms,
            buffered_seconds: cache.buffered_seconds,
            source_mbps,
            download_mbps,
            transfer_demanded: stats.transfer_demanded,
            buffering: cache.buffering,
            paused: cache.paused,
            playback_speed: cache.playback_speed,
            throttled: stats.parallel == 0,
        });
        Some(LinkHealth {
            source_mbps,
            download_mbps,
            transfer_demanded: Some(stats.transfer_demanded),
        })
    }

    fn error(&self) -> Option<PlaybackError> {
        match &self.transport {
            Transport::Http(session) => session.stats().error.map(media_error),
            Transport::Torrent(stream) => stream.stats().error.map(torrent_error),
            Transport::File => None,
        }
    }

    fn torrent(&self) -> Option<TorrentStats> {
        match &self.transport {
            Transport::Torrent(stream) => Some(stream.stats()),
            _ => None,
        }
    }

    fn stop(&mut self) {
        self.close_started = Some(tokio::time::Instant::now());
        if let Some(player) = &self.player {
            let _ = player.stop();
        }
    }

    fn close_session(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async {
            match &self.transport {
                Transport::Http(session) => session.close().await,
                Transport::Torrent(stream) => {
                    let _ = stream.close().await;
                }
                Transport::File => {}
            }
            self.cancel.cancel();
        })
    }

    fn close_player(mut self: Box<Self>) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            let started = self.close_started.unwrap_or_else(tokio::time::Instant::now);
            if let Some(player) = self.player.take() {
                let done = player.close();
                wait_for_close(done, self.events.take(), self.shutdown).await;
            }
            if self.diagnostics {
                crate::debug::log(format!("player close_ms={}", started.elapsed().as_millis()));
            }
        })
    }
}

/// Init options, including the bundled faces libass loads before any subtitle renders.
fn init_options(fonts: Option<&std::path::Path>) -> Vec<(String, String)> {
    fonts
        .and_then(std::path::Path::to_str)
        .map(|dir| {
            vec![
                ("sub-fonts-dir".into(), dir.into()),
                ("sub-font".into(), "Aspekta 400".into()),
            ]
        })
        .unwrap_or_default()
        .into_iter()
        .chain([("cache-pause-wait".into(), "5".into())])
        .collect()
}

async fn wait_for_close(
    done: mpsc::Receiver<Result<(), panorama_mpv::MpvError>>,
    events: Option<mpsc::Receiver<PlayerEvent>>,
    shutdown: bool,
) {
    loop {
        if shutdown
            || events.as_ref().is_none_or(|events| {
                events
                    .try_iter()
                    .any(|event| event == PlayerEvent::Shutdown)
            })
            || !matches!(done.try_recv(), Err(mpsc::TryRecvError::Empty))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn close_timeout_and_disconnect_release_the_host() {
        for completion in [Some(Err(panorama_mpv::MpvError::ShutdownTimeout)), None] {
            let (sender, done) = mpsc::channel();
            let (_events_sender, events) = mpsc::channel();
            if let Some(result) = completion {
                sender.send(result).unwrap();
            }
            drop(sender);
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(100),
                    wait_for_close(done, Some(events), false),
                )
                .await
                .is_ok()
            );
        }
    }

    #[test]
    fn init_options_are_all_accepted_by_the_mpv_host() {
        let names: Vec<_> = init_options(Some(std::path::Path::new("/fonts")))
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["sub-fonts-dir", "sub-font", "cache-pause-wait"]);
        for name in &names {
            assert!(
                panorama_mpv::EXTRA_OPTIONS.contains(&name.as_str()),
                "{name}"
            );
        }
    }

    #[test]
    fn runtime_properties_are_all_accepted_by_the_mpv_host() {
        let settings = crate::playback::output::PlaybackSettings::default();
        let output = crate::playback::output::properties(&settings, &[], true);
        let names: Vec<_> = output.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "audio-spdif",
                "audio-device",
                "audio-channels",
                "audio-spdif",
                "target-colorspace-hint",
                "target-prim",
                "target-trc"
            ]
        );
        for name in names.into_iter().chain(["mute", "sub-visibility"]) {
            assert!(panorama_mpv::PROPERTIES.contains(&name), "{name}");
        }
    }
}
