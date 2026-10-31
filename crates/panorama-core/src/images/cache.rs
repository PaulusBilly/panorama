use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use image::DynamicImage;
use sha2::{Digest, Sha256};

use super::{ImageError, ImageLoaderOptions, ImageUrl, decode};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(super) struct Cache {
    root: PathBuf,
    lock_path: PathBuf,
    budget: u64,
    body_cap: u64,
    index: Mutex<HashMap<PathBuf, Entry>>,
    local_generation: AtomicU64,
    #[cfg(test)]
    blocked_lock: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

struct Entry {
    bytes: u64,
    modified: SystemTime,
}

impl Cache {
    pub(super) fn new(options: &ImageLoaderOptions) -> Result<Self, ImageError> {
        fs::create_dir_all(&options.cache_dir).map_err(|_| ImageError::Cache)?;
        let root = fs::canonicalize(&options.cache_dir).map_err(|_| ImageError::Cache)?;
        if root.parent().is_none() {
            return Err(ImageError::Cache);
        }
        let mut lock_name = root.as_os_str().to_owned();
        lock_name.push(".images.lock");
        let cache = Self {
            root,
            lock_path: PathBuf::from(lock_name),
            budget: options.max_disk_bytes,
            body_cap: options.max_body_bytes,
            index: Mutex::new(HashMap::new()),
            local_generation: AtomicU64::new(0),
            #[cfg(test)]
            blocked_lock: Mutex::new(None),
        };
        cache.with_lock(|index, _| cache.prune(index))?;
        Ok(cache)
    }

    pub(super) fn key(url: &ImageUrl) -> String {
        format!("{:x}", Sha256::digest(url.0.as_str().as_bytes()))
    }

    fn path(&self, key: &str) -> PathBuf {
        self.root.join(&key[..2]).join(key)
    }

    pub(super) fn read(&self, key: &str) -> Result<(Option<DynamicImage>, u64), ImageError> {
        self.with_lock(|index, lock| {
            let generation = generation(lock)?;
            let path = self.path(key);
            let mut file = match OpenOptions::new().read(true).write(true).open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok((None, generation));
                }
                Err(_) => return Err(ImageError::Cache),
            };
            let mut bytes = Vec::new();
            (&mut file)
                .take(self.body_cap.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(|_| ImageError::Cache)?;
            let decoded = if bytes.len() as u64 > self.body_cap {
                Err(ImageError::TooLarge)
            } else {
                decode::decode(&bytes)
            };
            match decoded {
                Ok(image) => {
                    touch(&file, index)?;
                    Ok((Some(image), generation))
                }
                Err(_) => {
                    drop(file);
                    fs::remove_file(&path).map_err(|_| ImageError::Cache)?;
                    index.remove(&path);
                    Ok((None, generation))
                }
            }
        })
    }

    pub(super) fn local_generation(&self) -> u64 {
        self.local_generation.load(Ordering::SeqCst)
    }

    pub(super) fn invalidate(&self) {
        self.local_generation.fetch_add(1, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(super) fn on_blocked_lock(&self, hook: Box<dyn FnOnce() + Send>) {
        *self.blocked_lock.lock().unwrap() = Some(hook);
    }

    pub(super) fn insert(
        &self,
        key: &str,
        bytes: &[u8],
        epoch: u64,
        local_epoch: u64,
    ) -> Result<(), ImageError> {
        self.with_lock(|index, lock| {
            if self.local_generation() != local_epoch
                || generation(lock)? != epoch
                || bytes.len() as u64 > self.budget
            {
                return Ok(());
            }
            let path = self.path(key);
            let parent = path.parent().ok_or(ImageError::Cache)?;
            fs::create_dir_all(parent).map_err(|_| ImageError::Cache)?;
            let (temporary, mut file) = temporary(parent)?;
            file.write_all(bytes).map_err(|_| ImageError::Cache)?;
            touch(&file, index)?;
            file.sync_all().map_err(|_| ImageError::Cache)?;
            drop(file);
            fs::rename(&temporary.0, &path).map_err(|_| ImageError::Cache)?;
            let metadata = fs::metadata(&path).map_err(|_| ImageError::Cache)?;
            index.insert(
                path,
                Entry {
                    bytes: metadata.len(),
                    modified: metadata.modified().map_err(|_| ImageError::Cache)?,
                },
            );
            self.prune(index)
        })
    }

    pub(super) fn clear(&self) -> Result<(), ImageError> {
        self.with_lock(|index, lock| {
            let epoch = generation(lock)?.wrapping_add(1);
            lock.seek(SeekFrom::Start(0))
                .map_err(|_| ImageError::Cache)?;
            lock.write_all(&epoch.to_le_bytes())
                .map_err(|_| ImageError::Cache)?;
            lock.sync_all().map_err(|_| ImageError::Cache)?;
            for entry in fs::read_dir(&self.root).map_err(|_| ImageError::Cache)? {
                let entry = entry.map_err(|_| ImageError::Cache)?;
                if entry.file_type().map_err(|_| ImageError::Cache)?.is_dir() {
                    fs::remove_dir_all(entry.path()).map_err(|_| ImageError::Cache)?;
                } else {
                    fs::remove_file(entry.path()).map_err(|_| ImageError::Cache)?;
                }
            }
            index.clear();
            Ok(())
        })
    }

    fn with_lock<T>(
        &self,
        action: impl FnOnce(&mut HashMap<PathBuf, Entry>, &mut File) -> Result<T, ImageError>,
    ) -> Result<T, ImageError> {
        let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
        let mut lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_path)
            .map_err(|_| ImageError::Cache)?;
        #[cfg(test)]
        if let Some(hook) = self.blocked_lock.lock().unwrap().take() {
            match lock.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => {
                    hook();
                    lock.lock().map_err(|_| ImageError::Cache)?;
                }
                Err(_) => return Err(ImageError::Cache),
            }
        } else {
            lock.lock().map_err(|_| ImageError::Cache)?;
        }
        #[cfg(not(test))]
        lock.lock().map_err(|_| ImageError::Cache)?;
        *index = scan(&self.root)?;
        action(&mut index, &mut lock)
    }

    fn prune(&self, index: &mut HashMap<PathBuf, Entry>) -> Result<(), ImageError> {
        let mut total = index
            .values()
            .fold(0u64, |total, entry| total.saturating_add(entry.bytes));
        if total <= self.budget {
            return Ok(());
        }
        let watermark = (self.budget as u128 * 9 / 10) as u64;
        let mut oldest: Vec<_> = index
            .iter()
            .map(|(path, entry)| (entry.modified, path.clone(), entry.bytes))
            .collect();
        oldest.sort();
        for (_, path, bytes) in oldest {
            if total <= watermark {
                break;
            }
            fs::remove_file(&path).map_err(|_| ImageError::Cache)?;
            index.remove(&path);
            total = total.saturating_sub(bytes);
        }
        Ok(())
    }
}

fn scan(root: &Path) -> Result<HashMap<PathBuf, Entry>, ImageError> {
    let mut index = HashMap::new();
    for directory in fs::read_dir(root).map_err(|_| ImageError::Cache)? {
        let directory = directory.map_err(|_| ImageError::Cache)?;
        let prefix = directory.file_name().to_string_lossy().into_owned();
        if !directory
            .file_type()
            .map_err(|_| ImageError::Cache)?
            .is_dir()
            || !hex_name(&prefix, 2)
        {
            continue;
        }
        for file in fs::read_dir(directory.path()).map_err(|_| ImageError::Cache)? {
            let file = file.map_err(|_| ImageError::Cache)?;
            if !file.file_type().map_err(|_| ImageError::Cache)?.is_file() {
                continue;
            }
            let name = file.file_name().to_string_lossy().into_owned();
            if name.starts_with(".tmp-") {
                fs::remove_file(file.path()).map_err(|_| ImageError::Cache)?;
            } else if hex_name(&name, 64) && name.starts_with(&prefix) {
                let metadata = file.metadata().map_err(|_| ImageError::Cache)?;
                index.insert(
                    file.path(),
                    Entry {
                        bytes: metadata.len(),
                        modified: metadata.modified().map_err(|_| ImageError::Cache)?,
                    },
                );
            }
        }
    }
    Ok(index)
}

fn hex_name(name: &str, length: usize) -> bool {
    name.len() == length
        && name
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn generation(lock: &mut File) -> Result<u64, ImageError> {
    lock.seek(SeekFrom::Start(0))
        .map_err(|_| ImageError::Cache)?;
    if lock.metadata().map_err(|_| ImageError::Cache)?.len() == 0 {
        return Ok(0);
    }
    let mut bytes = [0; 8];
    lock.read_exact(&mut bytes).map_err(|_| ImageError::Cache)?;
    Ok(u64::from_le_bytes(bytes))
}

fn touch(file: &File, index: &HashMap<PathBuf, Entry>) -> Result<(), ImageError> {
    let latest = index.values().map(|entry| entry.modified).max();
    let now = SystemTime::now();
    let time = latest
        .and_then(|time| time.checked_add(Duration::from_millis(1)))
        .map_or(now, |time| time.max(now));
    file.set_modified(time).map_err(|_| ImageError::Cache)
}

struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn temporary(parent: &Path) -> Result<(Temporary, File), ImageError> {
    loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".tmp-{}-{id}", std::process::id()));
        match File::create_new(&path) {
            Ok(file) => return Ok((Temporary(path), file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(ImageError::Cache),
        }
    }
}
