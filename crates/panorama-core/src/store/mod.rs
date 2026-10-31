//! SQLite storage for opaque core state, cached resources, and preferences.
//!
//! Unix databases are created with mode 0600 and new directories with mode 0700.
//! On Windows, use [`default_path`] under `%LOCALAPPDATA%` to inherit the user's
//! profile ACL; arbitrary caller-supplied paths must have appropriate ACLs.
//! No Windows ACL editing is performed. Values are not encrypted at rest.
//! Share one `Store` across threads and finish core writes before signing out. Only one
//! `Store` per database may be open at a time, across processes: a second `open` returns
//! [`StoreError::Locked`] (enforced with an exclusive `<db>.lock` file).
//! SQLite schema versions are independent of `core:schema_version`.

mod error;
mod key;
mod migrations;
mod recovery;

pub use error::StoreError;
pub use key::{Key, KeyClass, MAX_COMPONENT_BYTES};
pub use migrations::SCHEMA_VERSION;

use std::{
    env,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

/// Maximum accepted size of one value: 16 MiB.
pub const MAX_VALUE_BYTES: usize = 16 * 1024 * 1024;
/// Cache lifetime in milliseconds: 30 days.
pub const CACHE_MAX_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// Maximum number of cache entries retained after pruning.
pub const CACHE_MAX_ENTRIES: usize = 2000;

/// Supplies Unix milliseconds for writes and corruption quarantine names.
pub trait Clock: Send + Sync {
    /// Returns the current Unix timestamp in milliseconds.
    fn now_ms(&self) -> i64;
}

impl<F: Fn() -> i64 + Send + Sync> Clock for F {
    fn now_ms(&self) -> i64 {
        self()
    }
}

/// One SQLite connection, serialized by a mutex; safe to share across threads.
pub struct Store {
    connection: Mutex<Connection>,
    cache_generation: AtomicU64,
    clock: Box<dyn Clock>,
    /// Database path for persistent stores; `None` in memory.
    path: Option<PathBuf>,
    /// Exclusive `<db>.lock` handle; released when the store is dropped.
    _lock: Option<File>,
}

/// Describes whether opening a store created or recovered its database.
#[derive(Debug, Eq, PartialEq)]
pub enum OpenOutcome {
    /// An existing database (including an empty file) was opened.
    Opened,
    /// A new database file was created.
    Created,
    /// A corrupt database was quarantined and replaced with an empty store.
    RecoveredFromCorruption {
        /// Path of the quarantined database; sidecars have `-wal`/`-shm` suffixes.
        quarantined: PathBuf,
    },
}

/// Stored bytes and their last-write timestamp.
#[derive(Debug, Eq, PartialEq)]
pub struct Entry {
    /// Opaque stored bytes.
    pub value: Vec<u8>,
    /// Unix milliseconds supplied by the store's clock at the last write.
    pub updated_at: i64,
}

/// Counts of cache rows removed by each pruning policy.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct PruneReport {
    /// Rows older than the cache lifetime.
    pub removed_by_age: usize,
    /// Additional oldest rows removed to meet the entry limit.
    pub removed_by_count: usize,
}

impl Store {
    /// Opens a persistent store using the system clock.
    pub fn open(path: &Path) -> Result<(Self, OpenOutcome), StoreError> {
        Self::open_with_clock(path, system_now_ms)
    }

    /// Opens a persistent store with an injected clock for deterministic writes.
    pub fn open_with_clock(
        path: &Path,
        clock: impl Clock + 'static,
    ) -> Result<(Self, OpenOutcome), StoreError> {
        let lock = recovery::lock(path)?;
        recovery::restrict_siblings(path)?;
        let existing = path.try_exists()?;
        let attempt: Result<(Connection, bool), StoreError> = (|| {
            if existing {
                preflight(path)?;
            }
            let created = recovery::prepare(path)?;
            let mut connection = Connection::open(path)?;
            migrations::version(&connection)?;
            configure(&connection)?;
            migrations::migrate(&mut connection)?;
            Ok((connection, created))
        })();
        let (connection, outcome) = match attempt {
            Ok((connection, created)) => (
                connection,
                if created {
                    OpenOutcome::Created
                } else {
                    OpenOutcome::Opened
                },
            ),
            Err(error) if existing && error.is_corruption() => {
                recovery::prepare(path)?;
                let quarantined = recovery::quarantine(path, clock.now_ms())?;
                recovery::prepare(path)?;
                let mut connection = Connection::open(path)?;
                configure(&connection)?;
                migrations::migrate(&mut connection)?;
                (
                    connection,
                    OpenOutcome::RecoveredFromCorruption { quarantined },
                )
            }
            Err(error) => return Err(error),
        };
        Ok((
            Self {
                connection: Mutex::new(connection),
                cache_generation: AtomicU64::new(0),
                clock: Box::new(clock),
                path: Some(path.to_path_buf()),
                _lock: Some(lock),
            },
            outcome,
        ))
    }

    /// Creates an ephemeral store using the system clock, suitable as a fallback.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::open_in_memory_with_clock(system_now_ms)
    }

    /// Creates an ephemeral store with an injected clock.
    pub fn open_in_memory_with_clock(clock: impl Clock + 'static) -> Result<Self, StoreError> {
        let mut connection = Connection::open_in_memory()?;
        configure(&connection)?;
        migrations::migrate(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            cache_generation: AtomicU64::new(0),
            clock: Box::new(clock),
            path: None,
            _lock: None,
        })
    }

    /// Reads opaque bytes; missing keys return `None`.
    pub fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.get_entry(key)?.map(|entry| entry.value))
    }

    /// Reads opaque bytes and the last-write Unix timestamp in milliseconds.
    pub fn get_entry(&self, key: &Key) -> Result<Option<Entry>, StoreError> {
        Ok(self
            .lock()
            .query_row(
                "SELECT value, updated_at FROM kv WHERE key = ?1",
                [key.as_str()],
                |row| {
                    Ok(Entry {
                        value: row.get(0)?,
                        updated_at: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    /// Inserts or overwrites a value, recording the injected clock's timestamp.
    pub fn set(&self, key: &Key, value: &[u8]) -> Result<(), StoreError> {
        check_size(value)?;
        let connection = self.lock();
        write(&connection, key, value, self.clock.now_ms())
    }

    pub(crate) fn cache_generation(&self) -> u64 {
        self.cache_generation.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn inject_read_failure(&self, key: &Key) {
        self.lock()
            .execute(
                "INSERT INTO kv(key, value, updated_at) VALUES (?1, X'00', 'invalid')",
                [key.as_str()],
            )
            .unwrap();
    }

    pub(crate) fn set_if_generation(
        &self,
        key: &Key,
        value: &[u8],
        generation: u64,
    ) -> Result<(), StoreError> {
        check_size(value)?;
        let connection = self.lock();
        if self.cache_generation() != generation {
            return Ok(());
        }
        write(&connection, key, value, self.clock.now_ms())
    }

    /// Deletes a key, succeeding even when the key does not exist.
    pub fn remove(&self, key: &Key) -> Result<(), StoreError> {
        self.lock()
            .execute("DELETE FROM kv WHERE key = ?1", [key.as_str()])?;
        Ok(())
    }

    pub(crate) fn clear_addon_catalogs(&self, addon: &str) -> Result<(), StoreError> {
        let prefix = format!("catalog:{addon}|");
        let connection = self.lock();
        connection.execute(
            "DELETE FROM kv WHERE substr(key, 1, length(?1)) = ?1",
            [&prefix],
        )?;
        self.cache_generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub(crate) fn get_or_insert(&self, key: &Key, value: &[u8]) -> Result<Vec<u8>, StoreError> {
        check_size(value)?;
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT OR IGNORE INTO kv(key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![key.as_str(), value, self.clock.now_ms()],
        )?;
        let result = transaction.query_row(
            "SELECT value FROM kv WHERE key = ?1",
            [key.as_str()],
            |row| row.get(0),
        )?;
        transaction.commit()?;
        Ok(result)
    }

    /// Applies all writes/deletions atomically, using one timestamp; `None` deletes.
    pub fn set_many(&self, items: &[(Key, Option<Vec<u8>>)]) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now_ms = self.clock.now_ms();
        for (key, value) in items {
            match value {
                Some(value) => {
                    check_size(value)?;
                    write(&transaction, key, value, now_ms)?;
                }
                None => {
                    transaction.execute("DELETE FROM kv WHERE key = ?1", [key.as_str()])?;
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Removes expired cache rows, then the oldest excess rows, in one transaction.
    pub fn prune_cache(&self, now_ms: i64) -> Result<PruneReport, StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed_by_age = transaction.execute(
            "DELETE FROM kv INDEXED BY kv_cache_updated_at WHERE (key GLOB 'catalog:*' OR key GLOB 'meta:*') AND updated_at < ?1",
            [now_ms.saturating_sub(CACHE_MAX_AGE_MS)],
        )?;
        let count: i64 = transaction.query_row(
            "SELECT count(*) FROM kv INDEXED BY kv_cache_updated_at WHERE key GLOB 'catalog:*' OR key GLOB 'meta:*'",
            [],
            |row| row.get(0),
        )?;
        let removed_by_count = transaction.execute(
            "DELETE FROM kv WHERE key IN (
                SELECT key FROM kv INDEXED BY kv_cache_updated_at WHERE key GLOB 'catalog:*' OR key GLOB 'meta:*'
                ORDER BY updated_at, key LIMIT ?1
            )",
            [count.saturating_sub(CACHE_MAX_ENTRIES as i64).max(0)],
        )?;
        transaction.commit()?;
        Ok(PruneReport {
            removed_by_age,
            removed_by_count,
        })
    }

    /// Deletes core and cache rows atomically, retaining preferences, truncates WAL, and
    /// deletes quarantined corrupt copies, which may still hold the previous session key.
    /// A blocked checkpoint returns an error even though the deletion has committed;
    /// retry after other connections release their readers.
    pub fn clear_on_sign_out(&self) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM kv WHERE key GLOB 'core:*' OR key GLOB 'catalog:*' OR key GLOB 'meta:*'",
            [],
        )?;
        transaction.commit()?;
        self.cache_generation.fetch_add(1, Ordering::AcqRel);
        checkpoint(&connection)?;
        if let Some(path) = &self.path {
            recovery::retain_newest(path, 0)?;
        }
        Ok(())
    }

    pub(crate) fn checkpoint(&self) -> Result<(), StoreError> {
        checkpoint(&self.lock())
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        match self.connection.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// Returns the per-user database path from environment variables, without creating it.
/// Returns `None` when no nonempty platform base directory is available.
pub fn default_path() -> Option<PathBuf> {
    default_path_from(|name| env::var_os(name).map(PathBuf::from))
}

fn default_path_from(get: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let base = |name| get(name).filter(|path| !path.as_os_str().is_empty());
    #[cfg(windows)]
    {
        base("LOCALAPPDATA").map(|path| path.join("Panorama").join("panorama.db"))
    }
    #[cfg(target_os = "macos")]
    {
        base("HOME").map(|path| path.join("Library/Application Support/Panorama/panorama.db"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        base("XDG_DATA_HOME")
            .or_else(|| base("HOME").map(|path| path.join(".local/share")))
            .map(|path| path.join("panorama/panorama.db"))
    }
}

fn preflight(path: &Path) -> Result<(), StoreError> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() == 0 {
        return Ok(());
    }
    let mut header = [0; 16];
    match file.read_exact(&mut header) {
        Ok(()) if &header == b"SQLite format 3\0" => {}
        Ok(()) => return Err(StoreError::Corrupt),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(StoreError::Corrupt);
        }
        Err(error) => return Err(error.into()),
    }
    drop(file);
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(Duration::from_millis(5000))?;
    migrations::version(&connection)?;
    let mut statement = connection.prepare("PRAGMA quick_check")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, String>(0)? != "ok" {
            return Err(StoreError::Corrupt);
        }
    }
    Ok(())
}

fn configure(connection: &Connection) -> Result<(), StoreError> {
    connection.busy_timeout(Duration::from_millis(5000))?;
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON;")?;
    Ok(())
}

fn checkpoint(connection: &Connection) -> Result<(), StoreError> {
    let busy: i64 =
        connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
    if busy != 0 {
        return Err(StoreError::CheckpointBusy);
    }
    Ok(())
}

fn check_size(value: &[u8]) -> Result<(), StoreError> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(StoreError::ValueTooLarge {
            size: value.len(),
            limit: MAX_VALUE_BYTES,
        });
    }
    Ok(())
}

fn write(connection: &Connection, key: &Key, value: &[u8], now_ms: i64) -> Result<(), StoreError> {
    connection.execute(
        "INSERT INTO kv(key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
        params![key.as_str(), value, now_ms],
    )?;
    Ok(())
}

fn system_now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().min(i64::MAX as u128) as i64,
        Err(error) => match i64::try_from(error.duration().as_millis()) {
            Ok(ms) => -ms,
            Err(_) => i64::MIN,
        },
    }
}

#[cfg(test)]
mod tests;
