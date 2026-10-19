//! Bounded memory/disk chunk cache ported from `desktop/main/media-cache.ts`.

mod disk;
use super::{MediaError, lock};
use bytes::Bytes;
use disk::Disk;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// Cache admission and disk reserve limits.
#[derive(Clone, Debug)]
pub struct MediaCacheOptions {
    /// Parent containing this process's private owner directory.
    pub directory: PathBuf,
    /// Budget shared by resident data and in-flight reservations.
    pub max_memory_bytes: u64,
    /// Budget for allocated chunk files plus directory overhead.
    pub max_disk_bytes: u64,
    /// Disk space left available to other applications.
    pub reserve_free_bytes: u64,
}

/// Current resident, allocated and reserved byte counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Resident completed chunks.
    pub memory_bytes: u64,
    /// Allocated disk files and directory overhead.
    pub disk_bytes: u64,
    /// Buffers reserved before allocation.
    pub reserved_bytes: u64,
}

struct Entry {
    data: Option<Bytes>,
    file: Option<PathBuf>,
    bytes: u64,
    allocated: u64,
}
struct State {
    options: MediaCacheOptions,
    disk: Option<Disk>,
    entries: HashMap<String, Entry>,
    order: VecDeque<String>,
    pins: HashMap<String, usize>,
    retired: HashSet<String>,
    reservations: HashMap<String, (u64, u64)>,
    next_lease: u64,
    stats: CacheStats,
    closed: bool,
}

impl State {
    fn trim(&mut self, needed: u64) -> bool {
        let keys: Vec<_> = self.order.iter().cloned().collect();
        for key in keys {
            if self.fits(needed) {
                break;
            }
            if self.pins.contains_key(&key) {
                continue;
            }
            if let Some(entry) = self.entries.get_mut(&key)
                && entry.data.take().is_some()
            {
                self.stats.memory_bytes -= entry.bytes;
                if entry.file.is_none() {
                    self.entries.remove(&key);
                    self.order.retain(|old| old != &key);
                }
            }
        }
        self.fits(needed)
    }
    fn fits(&self, needed: u64) -> bool {
        self.stats
            .memory_bytes
            .saturating_add(self.stats.reserved_bytes)
            .saturating_add(needed)
            <= self.options.max_memory_bytes
    }
    fn remove(&mut self, key: &str) {
        if let Some(entry) = self.entries.remove(key) {
            if let Some(file) = entry.file {
                let _ = fs::remove_file(file);
                self.stats.disk_bytes -= entry.allocated;
            }
            if entry.data.is_some() {
                self.stats.memory_bytes -= entry.bytes;
            }
            self.order.retain(|old| old != key);
        }
    }
    fn release(&mut self, key: &str, lease: u64) {
        if self
            .reservations
            .get(key)
            .is_some_and(|(_, owner)| *owner == lease)
            && let Some((bytes, _)) = self.reservations.remove(key)
        {
            self.stats.reserved_bytes -= bytes;
        }
    }
    fn reserve(&mut self, key: &str, bytes: u64) -> Result<Option<u64>, MediaError> {
        if bytes > self.options.max_memory_bytes || bytes > super::range::MAX_SAFE_INTEGER {
            return Err(MediaError::ChunkTooLarge);
        }
        if self.closed {
            return Err(MediaError::Closed);
        }
        if self.reservations.contains_key(key) || !self.trim(bytes) {
            return Ok(None);
        }
        self.next_lease = self
            .next_lease
            .checked_add(1)
            .ok_or(MediaError::Admission)?;
        self.reservations
            .insert(key.to_owned(), (bytes, self.next_lease));
        self.stats.reserved_bytes += bytes;
        Ok(Some(self.next_lease))
    }
    fn put(&mut self, key: &str, bytes: Bytes) -> Result<(), MediaError> {
        let write_key = format!("write:{key}");
        if self.closed
            || self.reservations.contains_key(&write_key)
            || !self.trim(bytes.len() as u64)
        {
            return Err(MediaError::Admission);
        }
        let lease = self
            .reserve(&write_key, bytes.len() as u64)?
            .ok_or(MediaError::Admission)?;
        let result = self.put_reserved(key, bytes);
        self.release(&write_key, lease);
        result
    }
    fn put_reserved(&mut self, key: &str, bytes: Bytes) -> Result<(), MediaError> {
        if self.entries.contains_key(key) {
            return Ok(());
        }
        if self.retired.iter().any(|prefix| key.starts_with(prefix)) {
            return Err(MediaError::Closed);
        }
        let length = bytes.len() as u64;
        let mut entry = Entry {
            bytes: length,
            data: None,
            file: None,
            allocated: 0,
        };
        if let Some(allocated) = self
            .disk
            .as_ref()
            .and_then(|disk| disk.allocation(length, self.options.reserve_free_bytes))
        {
            let keys: Vec<_> = self.order.iter().cloned().collect();
            for old in keys {
                if self.stats.disk_bytes.saturating_add(allocated) <= self.options.max_disk_bytes {
                    break;
                }
                if !self.pins.contains_key(&old) {
                    self.remove(&old);
                }
            }
            if self.stats.disk_bytes.saturating_add(allocated) <= self.options.max_disk_bytes
                && let Some(disk) = &self.disk
                && let Ok((file, actual)) = disk.write(key, &bytes, allocated)
            {
                if self.stats.disk_bytes.saturating_add(actual) <= self.options.max_disk_bytes {
                    self.stats.disk_bytes += actual;
                    entry.allocated = actual;
                    entry.file = Some(file);
                } else {
                    let _ = fs::remove_file(file);
                }
            }
        }
        // TS counts the write reservation again while deciding memory residency.
        if self.trim(length) {
            entry.data = Some(bytes);
            self.stats.memory_bytes += length;
        }
        if entry.data.is_none() && entry.file.is_none() {
            return Err(MediaError::Admission);
        }
        self.entries.insert(key.to_owned(), entry);
        self.order.push_back(key.to_owned());
        Ok(())
    }
    fn close(&mut self) {
        self.closed = true;
        self.entries.clear();
        self.order.clear();
        self.pins.clear();
        self.retired.clear();
        self.reservations.clear();
        self.stats = CacheStats::default();
        self.disk.take();
    }
}

/// Serialized cache; disk operations run on Tokio's blocking pool.
#[derive(Clone)]
pub struct MediaCache {
    state: Arc<Mutex<State>>,
}

impl MediaCache {
    /// Creates a private owner directory, falling back to memory if unavailable.
    pub async fn open(options: MediaCacheOptions) -> Result<Self, MediaError> {
        if options.max_memory_bytes < super::proxy::CHUNK_BYTES {
            return Err(MediaError::MemoryBudget);
        }
        tokio::task::spawn_blocking(move || {
            let disk = Disk::open(&options.directory).ok();
            let disk_bytes = if disk.is_some() { 4096 } else { 0 };
            Self {
                state: Arc::new(Mutex::new(State {
                    options,
                    disk,
                    entries: Default::default(),
                    order: Default::default(),
                    pins: Default::default(),
                    retired: Default::default(),
                    reservations: Default::default(),
                    next_lease: 0,
                    stats: CacheStats {
                        disk_bytes,
                        ..Default::default()
                    },
                    closed: false,
                })),
            }
        })
        .await
        .map_err(|_| MediaError::Transport)
    }
    /// Tests membership without reading disk.
    pub fn has(&self, key: &str) -> bool {
        let state = lock(&self.state);
        !state.closed && state.entries.contains_key(key)
    }
    /// Reads and touches a completed entry. Disk reads require available memory.
    pub async fn get(&self, key: &str) -> Option<Bytes> {
        let state = self.state.clone();
        let key = key.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut state = lock(&state);
            if state.closed {
                return None;
            }
            let entry = state.entries.get(&key)?;
            let (data, file, length) = (entry.data.clone(), entry.file.clone(), entry.bytes);
            state.order.retain(|old| old != &key);
            state.order.push_back(key.clone());
            if data.is_some() {
                return data;
            }
            let file = file?;
            if !state.reservations.contains_key(&key) && !state.trim(length) {
                return None;
            }
            match fs::read(file) {
                Ok(bytes) if bytes.len() as u64 == length => Some(Bytes::from(bytes)),
                _ => {
                    state.remove(&key);
                    None
                }
            }
        })
        .await
        .ok()
        .flatten()
    }
    /// Admits a completed chunk, respecting pins, write reservations and disk budget.
    pub async fn put(&self, key: &str, bytes: Bytes) -> Result<(), MediaError> {
        let state = self.state.clone();
        let key = key.to_owned();
        tokio::task::spawn_blocking(move || lock(&state).put(&key, bytes))
            .await
            .map_err(|_| MediaError::Transport)?
    }
    /// Adds one reader reference, including before an entry exists.
    pub fn pin(&self, key: &str) {
        *lock(&self.state).pins.entry(key.to_owned()).or_default() += 1;
    }
    /// Releases one reference and removes a retired entry after its last reader.
    pub fn unpin(&self, key: &str) {
        let mut state = lock(&self.state);
        if let Some(count) = state.pins.get_mut(key)
            && *count > 1
        {
            *count -= 1;
            return;
        }
        state.pins.remove(key);
        if state.retired.iter().any(|prefix| key.starts_with(prefix)) {
            state.remove(key);
        }
    }
    /// Reserves memory for one owner; None indicates temporary cache pressure.
    pub fn reserve(&self, key: &str, bytes: u64) -> Result<Option<u64>, MediaError> {
        lock(&self.state).reserve(key, bytes)
    }
    /// Releases an in-flight allocation reservation only when its lease matches.
    pub fn release(&self, key: &str, lease: u64) {
        lock(&self.state).release(key, lease);
    }
    /// Retires all entries with a session prefix, preserving active reader pins.
    pub async fn remove_session(&self, prefix: &str) {
        let state = self.state.clone();
        let prefix = prefix.to_owned();
        let _ = tokio::task::spawn_blocking(move || {
            let mut state = lock(&state);
            state.retired.insert(prefix.clone());
            let keys: Vec<_> = state
                .entries
                .keys()
                .filter(|key| key.starts_with(&prefix) && !state.pins.contains_key(*key))
                .cloned()
                .collect();
            for key in keys {
                state.remove(&key);
            }
        })
        .await;
    }
    /// Returns current budget accounting.
    pub fn stats(&self) -> CacheStats {
        lock(&self.state).stats
    }
    /// Removes only this cache's owner directory after queued operations finish.
    pub async fn close(&self) {
        let state = self.state.clone();
        let _ = tokio::task::spawn_blocking(move || lock(&state).close()).await;
    }
    pub(crate) fn close_now(&self) {
        lock(&self.state).close();
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests;
