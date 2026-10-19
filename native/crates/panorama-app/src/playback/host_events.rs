use super::*;

impl Runner {
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
            if let PlayerEvent::DemuxerCacheState(cache) = &event
                && let Some(active) = &self.active
            {
                active.cache(
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
