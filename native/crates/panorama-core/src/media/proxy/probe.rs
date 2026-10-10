//! Probing and warm-up ported from `desktop/main/media-proxy.ts`.

use super::{
    CHUNK_BYTES, MediaWarmResult,
    session::{Mode, Session, cancelled},
};
use super::{MediaProxyEvent, ProbeResult};
use crate::media::{
    MediaError, lock,
    range::{header, media_validator, validate_media_range},
};
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;

impl Session {
    pub async fn prepare(self: &Arc<Self>) -> Result<Mode, MediaError> {
        let _guard = self.prepare_lock.lock().await;
        if self.stopped() {
            return Err(lock(&self.state)
                .error
                .clone()
                .unwrap_or(MediaError::Closed));
        }
        if self.unreachable() {
            return Err(MediaError::Transport);
        }
        if let Some(mode) = lock(&self.state).mode {
            return Ok(mode);
        }
        let started = Instant::now();
        let result = self.probe().await;
        self.emit(MediaProxyEvent::Probe {
            result: match result {
                Ok(Mode::Ranged) => ProbeResult::Ranged,
                Ok(Mode::Passthrough) => ProbeResult::Passthrough,
                Err(_) => ProbeResult::Failed,
            },
            status: match &result {
                Ok(Mode::Ranged) => Some(206),
                Ok(Mode::Passthrough) => Some(200),
                Err(MediaError::Status(status)) => Some(*status),
                _ => None,
            },
            ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            error: result.as_ref().err().cloned(),
        });
        if let Err(error) = &result {
            let mut state = lock(&self.state);
            state.error = Some(error.clone());
            if matches!(error, MediaError::Transport | MediaError::Representation(_)) {
                state.unreachable_at = Some(Instant::now());
            }
        }
        result
    }
    async fn probe(self: &Arc<Self>) -> Result<Mode, MediaError> {
        for attempt in 0..=6 {
            self.cooldown().await?;
            let response = self
                .request(
                    self.original.clone(),
                    0,
                    CHUNK_BYTES - 1,
                    None,
                    Duration::from_secs(90),
                )
                .await?;
            if matches!(response.status, 429 | 503) {
                self.throttle(header(&response.headers, "retry-after"));
                if attempt == 6 {
                    return Err(MediaError::Status(response.status));
                }
                continue;
            }
            if response.status == 206 {
                let range = validate_media_range(&response.headers, 0, CHUNK_BYTES - 1, None)?;
                {
                    let mut state = lock(&self.state);
                    state.size = Some(range.total);
                    state.source = response.final_source.clone();
                    state.validator = media_validator(&response.headers);
                    state.content_type = header(&response.headers, "content-type")
                        .unwrap_or("application/octet-stream")
                        .to_owned();
                    state.mode = Some(Mode::Ranged);
                    state.error = None;
                    state.unreachable_at = None;
                }
                self.chunk(0, Some(response));
                return Ok(Mode::Ranged);
            }
            if (200..300).contains(&response.status) {
                lock(&self.state).mode = Some(Mode::Passthrough);
                return Ok(Mode::Passthrough);
            }
            return Err(MediaError::Status(response.status));
        }
        Err(MediaError::Transport)
    }
    pub async fn warm(self: &Arc<Self>) -> MediaWarmResult {
        match self.prepare().await {
            Ok(Mode::Ranged) => {
                let last = lock(&self.state).size.unwrap_or(1).div_ceil(CHUNK_BYTES) - 1;
                for index in 1..3.min(last + 1) {
                    self.chunk(index, None);
                }
                if last >= 3 {
                    let session = self.clone();
                    self.spawn(async move {
                        loop {
                            if session.stopped() { return; }
                            let pending = { let state = lock(&session.state); state.chunks.values().filter(|chunk| chunk.running.load(std::sync::atomic::Ordering::SeqCst)).count() };
                            if pending < lock(&session.state).parallel() { session.chunk(last, None); return; }
                            tokio::select! { _ = cancelled(session.stop_signal.subscribe()) => return, _ = tokio::time::sleep(Duration::from_millis(25)) => {} }
                        }
                    });
                }
                MediaWarmResult {
                    ready: true,
                    error: None,
                    unreachable: false,
                }
            }
            Ok(Mode::Passthrough) => MediaWarmResult {
                ready: true,
                error: None,
                unreachable: false,
            },
            Err(error) => MediaWarmResult {
                ready: false,
                error: Some(error),
                unreachable: self.unreachable(),
            },
        }
    }
}
