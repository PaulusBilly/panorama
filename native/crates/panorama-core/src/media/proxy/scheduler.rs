//! Chunk scheduling and readers ported from `desktop/main/media-proxy.ts`.

use super::{
    CHUNK_BYTES,
    session::{Chunk, ChunkState, Reader, Session, cancelled},
};
use crate::media::{MediaError, fetch::FetchResponse, lock};
use bytes::{Bytes, BytesMut};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Notify, watch},
    time::Instant,
};

impl Session {
    pub fn needed(&self, index: u64) -> bool {
        lock(&self.state)
            .readers
            .values()
            .any(|reader| reader.position / CHUNK_BYTES == index)
    }
    pub fn chunk(self: &Arc<Self>, index: u64, initial: Option<FetchResponse>) -> Arc<Chunk> {
        let mut state = lock(&self.state);
        if let Some(chunk) = state.chunks.get(&index) {
            return chunk.clone();
        }
        if state
            .chunks
            .values()
            .filter(|chunk| chunk.running.load(Ordering::SeqCst))
            .count()
            >= state.parallel()
            && state
                .readers
                .values()
                .any(|reader| reader.position / CHUNK_BYTES == index)
        {
            for other_index in &state.chunk_order {
                let Some(other) = state.chunks.get(other_index) else {
                    continue;
                };
                if state
                    .readers
                    .values()
                    .any(|reader| reader.position / CHUNK_BYTES == *other_index)
                    || !other.running.load(Ordering::SeqCst)
                    || lock(&other.data).complete.is_some()
                {
                    continue;
                }
                other.stop.send_replace(true);
                other.notify.notify_waiters();
                break;
            }
        }
        let (stop, _) = watch::channel(false);
        let chunk = Arc::new(Chunk {
            data: std::sync::Mutex::new(ChunkState {
                data: BytesMut::new(),
                complete: None,
                error: None,
            }),
            notify: Notify::new(),
            stop,
            key: self.key(index),
            cache: self.cache.clone(),
            lease: std::sync::Mutex::new(None),
            running: AtomicBool::new(true),
        });
        state.chunks.insert(index, chunk.clone());
        state.chunk_order.push_back(index);
        drop(state);
        let session = self.clone();
        let work = chunk.clone();
        self.spawn(async move {
            let result = tokio::select! {
                _ = cancelled(work.stop.subscribe()) => Err(MediaError::Closed),
                _ = cancelled(session.stop_signal.subscribe()) => Err(MediaError::Closed),
                result = session.load(index, &work, initial) => result,
            };
            if let Err(error) = result {
                lock(&work.data).error = Some(error.clone());
                if !matches!(error, MediaError::Admission | MediaError::Closed) {
                    session.fail(error);
                }
                let mut state = lock(&session.state);
                if state
                    .chunks
                    .get(&index)
                    .is_some_and(|entry| Arc::ptr_eq(entry, &work))
                {
                    state.chunks.remove(&index);
                    state.chunk_order.retain(|old| *old != index);
                }
            }
            work.running.store(false, Ordering::SeqCst);
            work.notify.notify_waiters();
            session.schedule();
        });
        chunk
    }
    async fn load(
        self: &Arc<Self>,
        index: u64,
        chunk: &Arc<Chunk>,
        initial: Option<FetchResponse>,
    ) -> Result<(), MediaError> {
        let start = index * CHUNK_BYTES;
        let length = lock(&self.state)
            .size
            .unwrap_or(0)
            .min(start + CHUNK_BYTES)
            .saturating_sub(start);
        self.admit(index, chunk, length, initial.is_some()).await?;
        if let Some(cached) = self.cache.get(&chunk.key).await {
            lock(&chunk.data).complete = Some(cached);
            chunk.notify.notify_waiters();
            return Ok(());
        }
        lock(&chunk.data).data = BytesMut::with_capacity(length as usize);
        self.download(index, chunk, initial, length).await?;
        let bytes = {
            let mut data = lock(&chunk.data);
            let bytes = std::mem::take(&mut data.data).freeze();
            data.complete = Some(bytes.clone());
            bytes
        };
        chunk.notify.notify_waiters();
        if !self.stopped()
            && !*chunk.stop.borrow()
            && self.cache.put(&chunk.key, bytes).await.is_err()
        {
            let mut state = lock(&self.state);
            state.prefetch = false;
            state.demand = 1;
        }
        Ok(())
    }
    async fn admit(
        &self,
        index: u64,
        chunk: &Chunk,
        length: u64,
        opening: bool,
    ) -> Result<(), MediaError> {
        loop {
            if let Some(lease) = self.cache.reserve(&chunk.key, length)? {
                *lock(&chunk.lease) = Some(lease);
                return Ok(());
            }
            lock(&self.state).prefetch = false;
            if !opening && !self.needed(index) {
                return Err(MediaError::Admission);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    pub fn schedule(self: &Arc<Self>) {
        if self.stopped() {
            return;
        }
        let mut state = lock(&self.state);
        let Some(size) = state.size else {
            return;
        };
        if state.readers.is_empty() {
            if !state.ever_served || state.idle_armed {
                return;
            }
            state.idle_armed = true;
            state.idle_generation += 1;
            let generation = state.idle_generation;
            drop(state);
            let weak = Arc::downgrade(self);
            self.spawn(async move {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if let Some(session) = weak.upgrade() {
                    let mut state = lock(&session.state);
                    if state.readers.is_empty() && state.idle_generation == generation {
                        state.idle_armed = false;
                        for chunk in state.chunks.values() {
                            chunk.stop.send_replace(true);
                            chunk.notify.notify_waiters();
                        }
                        state.chunks.clear();
                        state.chunk_order.clear();
                    }
                }
            });
            return;
        }
        state.idle_generation += 1;
        state.idle_armed = false;
        let mut readers: Vec<_> = state
            .readers
            .iter()
            .map(|(id, reader)| (*id, reader.position / CHUNK_BYTES, reader.foreground))
            .collect();
        readers.sort_by_key(|(id, _, foreground)| (!foreground, *id));
        let window = state.window;
        let removed: Vec<_> = state
            .chunks
            .iter()
            .filter_map(|(index, chunk)| {
                let wanted = readers.iter().any(|(_, first, foreground)| {
                    *index >= first.saturating_sub(1)
                        && *index <= first + if *foreground { window } else { 0 }
                });
                let needed = readers.iter().any(|(_, first, _)| first == index);
                let complete = lock(&chunk.data).complete.is_some();
                (!wanted
                    || (complete && !needed && (self.cache.has(&chunk.key) || !state.prefetch)))
                    .then_some(*index)
            })
            .collect();
        for index in removed {
            if let Some(chunk) = state.chunks.remove(&index) {
                state.chunk_order.retain(|old| *old != index);
                chunk.stop.send_replace(true);
                chunk.notify.notify_waiters();
            }
        }
        let paused_until = state.paused_until;
        if Instant::now() < paused_until {
            if state.schedule_armed {
                return;
            }
            state.schedule_armed = true;
            drop(state);
            let weak = Arc::downgrade(self);
            self.spawn(async move {
                tokio::time::sleep_until(paused_until).await;
                if let Some(session) = weak.upgrade() {
                    lock(&session.state).schedule_armed = false;
                    session.schedule();
                }
            });
            return;
        }
        let mut pending = state
            .chunks
            .values()
            .filter(|chunk| chunk.running.load(Ordering::SeqCst))
            .count();
        let parallel = state.parallel();
        let mut wanted = Vec::new();
        for offset in 0..=window {
            for (_, first, foreground) in &readers {
                if pending >= parallel {
                    break;
                }
                if offset > 0 && (!foreground || !state.prefetch) {
                    continue;
                }
                let index = first + offset;
                if index < size.div_ceil(CHUNK_BYTES)
                    && !state.chunks.contains_key(&index)
                    && !self.cache.has(&self.key(index))
                    && !wanted.contains(&index)
                {
                    wanted.push(index);
                    pending += 1;
                }
            }
            if pending >= parallel {
                break;
            }
        }
        drop(state);
        for index in wanted {
            self.chunk(index, None);
        }
    }
    pub fn add_reader(self: &Arc<Self>, start: u64) -> Result<ReadGuard, MediaError> {
        let mut state = lock(&self.state);
        if state.readers.len() >= 8 {
            return Err(MediaError::Status(503));
        }
        let id = state.next_reader;
        state.next_reader += 1;
        let foreground = state.readers.is_empty();
        state.readers.insert(
            id,
            Reader {
                position: start,
                foreground,
            },
        );
        state.ever_served = true;
        Ok(ReadGuard {
            session: self.clone(),
            id,
            pin: None,
            position: start,
        })
    }
}

pub(super) struct ReadGuard {
    pub session: Arc<Session>,
    pub id: u64,
    pin: Option<String>,
    position: u64,
}
impl ReadGuard {
    pub async fn read(&mut self, end: u64) -> Result<Option<Bytes>, MediaError> {
        if self.position > end {
            return Ok(None);
        }
        if self.session.stopped() {
            return Err(lock(&self.session.state)
                .error
                .clone()
                .unwrap_or(MediaError::Closed));
        }
        let index = self.position / CHUNK_BYTES;
        let key = self.session.key(index);
        if self.pin.as_ref() != Some(&key) {
            if let Some(old) = self.pin.take() {
                self.session.cache.unpin(&old);
            }
            self.session.cache.pin(&key);
            self.pin = Some(key);
        }
        self.session.schedule();
        let chunk = self.session.chunk(index, None);
        let offset = (self.position % CHUNK_BYTES) as usize;
        let expected_length = lock(&self.session.state)
            .size
            .unwrap_or(0)
            .saturating_sub(index * CHUNK_BYTES)
            .min(CHUNK_BYTES) as usize;
        loop {
            let notified = chunk.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let result = {
                let data = lock(&chunk.data);
                let bytes: &[u8] = data.complete.as_deref().unwrap_or(&data.data);
                if bytes.len() > offset {
                    let length = (bytes.len() - offset)
                        .min(65536)
                        .min((end - self.position + 1) as usize);
                    if offset + length == expected_length && data.complete.is_none() {
                        data.error.clone().map(Err)
                    } else {
                        Some(Ok(Bytes::copy_from_slice(&bytes[offset..offset + length])))
                    }
                } else {
                    data.error.clone().map(Err)
                }
            };
            if let Some(result) = result {
                let bytes = result
                    .map_err(|error| lock(&self.session.state).error.clone().unwrap_or(error))?;
                self.position += bytes.len() as u64;
                if let Some(reader) = lock(&self.session.state).readers.get_mut(&self.id) {
                    reader.position = self.position;
                }
                return Ok(Some(bytes));
            }
            tokio::select! {
                _ = cancelled(self.session.stop_signal.subscribe()) => return Err(lock(&self.session.state).error.clone().unwrap_or(MediaError::Closed)),
                _ = cancelled(chunk.stop.subscribe()) => return Err(lock(&self.session.state).error.clone().unwrap_or(MediaError::Closed)),
                _ = notified => {},
            }
        }
    }
}
impl Drop for ReadGuard {
    fn drop(&mut self) {
        if let Some(key) = self.pin.take() {
            self.session.cache.unpin(&key);
        }
        {
            let mut state = lock(&self.session.state);
            let foreground = state
                .readers
                .remove(&self.id)
                .is_some_and(|reader| reader.foreground);
            if foreground
                && let Some(id) = state.readers.keys().min().copied()
                && let Some(reader) = state.readers.get_mut(&id)
            {
                reader.foreground = true;
            }
        }
        self.session.schedule();
    }
}

#[cfg(test)]
mod tests;
