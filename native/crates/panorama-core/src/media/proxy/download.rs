//! Streaming retries and link refresh ported from `desktop/main/media-proxy.ts`.

use super::MediaProxyEvent;
use super::{
    CHUNK_BYTES,
    session::{Chunk, Session, cancelled},
};
use crate::media::{
    MediaError,
    fetch::FetchResponse,
    lock,
    range::{header, media_validator, validate_media_range},
};
use futures::StreamExt;
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;

struct Active(Arc<Session>);
impl Drop for Active {
    fn drop(&mut self) {
        lock(&self.0.state).active -= 1;
    }
}

impl Session {
    pub async fn download(
        self: &Arc<Self>,
        index: u64,
        chunk: &Arc<Chunk>,
        mut initial: Option<FetchResponse>,
        length: u64,
    ) -> Result<(), MediaError> {
        loop {
            {
                let mut state = lock(&self.state);
                if state.active < state.parallel() {
                    state.active += 1;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let _active = Active(self.clone());
        let start = index * CHUNK_BYTES;
        let end = start + length - 1;
        for attempt in 0_u32..=6 {
            self.cooldown().await?;
            if attempt > 0 && lock(&chunk.data).data.len() as u64 == length {
                lock(&chunk.data).data.clear();
            }
            let received = lock(&chunk.data).data.len() as u64;
            let (source, validator, total, generation) = {
                let state = lock(&self.state);
                (
                    state.source.clone(),
                    state.validator.clone(),
                    state.size,
                    state.generation,
                )
            };
            let expected_start = start + received;
            let response = match initial.take() {
                Some(response) => Ok(response),
                None => {
                    self.request(
                        source,
                        expected_start,
                        end,
                        validator.clone(),
                        self.stall_timeout,
                    )
                    .await
                }
            };
            let mut retry_after = None;
            let result = async {
                let mut response = response?;
                retry_after = header(&response.headers, "retry-after").map(str::to_owned);
                if response.status != 206 {
                    return Err(if response.status == 200 {
                        MediaError::Representation("Media representation changed")
                    } else {
                        MediaError::Status(response.status)
                    });
                }
                validate_media_range(&response.headers, expected_start, end, total)?;
                if validator.is_some() && media_validator(&response.headers) != validator {
                    return Err(MediaError::Representation("Media validator changed"));
                }
                if attempt > 0 && validator.is_none() {
                    return Err(MediaError::Representation("Cannot resume unverified media"));
                }
                loop {
                    let next = tokio::time::timeout(self.stall_timeout, response.body.next())
                        .await
                        .map_err(|_| MediaError::Timeout)?;
                    let Some(part) = next else {
                        break;
                    };
                    let part = part?;
                    {
                        let mut data = lock(&chunk.data);
                        if part.len() as u64 > length - data.data.len() as u64 {
                            return Err(MediaError::Representation("Overlong media range"));
                        }
                        data.data.extend_from_slice(&part);
                    }
                    lock(&self.state)
                        .samples
                        .push_back((Instant::now(), part.len()));
                    chunk.notify.notify_waiters();
                }
                if lock(&chunk.data).data.len() as u64 != length {
                    return Err(MediaError::Transport);
                }
                lock(&self.state).cooldown = self.base_cooldown;
                Ok(())
            }
            .await;
            let error = match result {
                Ok(()) => return Ok(()),
                Err(error) => error,
            };
            if self.stopped() || *chunk.stop.borrow() {
                return Err(MediaError::Closed);
            }
            let refresh = matches!(
                error,
                MediaError::Status(401 | 403 | 404 | 410)
                    | MediaError::Representation(
                        "Media representation changed"
                            | "Media validator changed"
                            | "Media range changed"
                    )
            );
            if matches!(error, MediaError::Representation(_)) && !refresh {
                return Err(error);
            }
            if refresh && let Err(failure) = self.reresolve(generation).await {
                self.fail(failure.clone());
                return Err(failure);
            }
            let throttled = matches!(error, MediaError::Timeout | MediaError::Status(429 | 503));
            if throttled {
                self.throttle(retry_after.as_deref());
                let state = lock(&self.state);
                let mut surplus = state.active.saturating_sub(state.parallel());
                for other_index in &state.chunk_order {
                    if surplus == 0 {
                        break;
                    }
                    let Some(other) = state.chunks.get(other_index) else {
                        continue;
                    };
                    if *other_index == index
                        || state
                            .readers
                            .values()
                            .any(|reader| reader.position / CHUNK_BYTES == *other_index)
                        || lock(&other.data).complete.is_some()
                    {
                        continue;
                    }
                    other.stop.send_replace(true);
                    other.notify.notify_waiters();
                    surplus -= 1;
                }
            }
            let parallel = lock(&self.state).parallel();
            self.emit(MediaProxyEvent::RangeRetry {
                index,
                attempt,
                status: match error {
                    MediaError::Status(status) => Some(status),
                    _ => None,
                },
                stalled: error == MediaError::Timeout,
                parallel,
            });
            let surplus_prefetch = {
                let state = lock(&self.state);
                throttled
                    && !state
                        .readers
                        .values()
                        .any(|reader| reader.position / CHUNK_BYTES == index)
                    && state
                        .chunks
                        .values()
                        .filter(|entry| entry.running.load(std::sync::atomic::Ordering::SeqCst))
                        .count()
                        > state.parallel()
            };
            if surplus_prefetch {
                return Err(MediaError::Closed);
            }
            if attempt == 6 {
                return Err(error);
            }
            self.cooldown().await?;
            if !throttled {
                tokio::time::sleep(Duration::from_millis(500 * 2_u64.pow(attempt.min(3)))).await;
            }
        }
        Err(MediaError::Transport)
    }
    pub(super) async fn reresolve(&self, generation: u64) -> Result<(), MediaError> {
        let _guard = tokio::select! { _ = cancelled(self.stop_signal.subscribe()) => return Err(MediaError::Closed), guard = self.resolve_lock.lock() => guard };
        if self.stopped() {
            return Err(lock(&self.state)
                .error
                .clone()
                .unwrap_or(MediaError::Closed));
        }
        if lock(&self.state).generation != generation {
            return Ok(());
        }
        let last = {
            let mut state = lock(&self.state);
            while state
                .resolves
                .front()
                .is_some_and(|time| time.elapsed() >= Duration::from_secs(600))
            {
                state.resolves.pop_front();
            }
            if state.resolves.len() >= 3 {
                return Err(MediaError::ResolveLimit);
            }
            state.resolves.back().copied()
        };
        if let Some(last) = last {
            tokio::select! { _ = cancelled(self.stop_signal.subscribe()) => return Err(MediaError::Closed), _ = tokio::time::sleep_until(last + Duration::from_secs(30)) => {} }
        }
        lock(&self.state).resolves.push_back(Instant::now());
        let fresh = if let Some(resolver) = &self.resolver {
            tokio::select! { _ = cancelled(self.stop_signal.subscribe()) => return Err(MediaError::Closed), result = tokio::time::timeout(Duration::from_secs(90), resolver.resolve(self.original.clone())) => result.map_err(|_| MediaError::ResolverFailed)?.map_err(|_| MediaError::ResolverFailed)? }
        } else {
            self.original.clone()
        };
        let response = self
            .request(fresh, 0, 0, None, Duration::from_secs(90))
            .await?;
        if response.status != 206 {
            return Err(MediaError::Status(response.status));
        }
        let (total, validator) = {
            let state = lock(&self.state);
            (state.size, state.validator.clone())
        };
        let range = validate_media_range(&response.headers, 0, 0, None)?;
        if Some(range.total) != total {
            return Err(MediaError::Representation("Resolved media size changed"));
        }
        // TS refuses refresh without a validator, even when the size matches.
        if validator.is_none() || media_validator(&response.headers) != validator {
            return Err(MediaError::Representation("Media identity changed"));
        }
        let mut state = lock(&self.state);
        state.source = response.final_source;
        state.generation += 1;
        Ok(())
    }
}
