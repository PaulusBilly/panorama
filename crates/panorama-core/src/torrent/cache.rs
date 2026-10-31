use super::{DiskError, TorrentError, directory::OwnedDir, lock};
use fs4::fs_std::FileExt;
use librqbit::{
    ManagedTorrentShared, TorrentMetadata,
    storage::{StorageFactory, StorageFactoryExt, TorrentStorage},
};
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub(super) const BLOCK: u64 = 64 * 1024;

pub(super) struct Budget {
    pub used: u64,
    pub limit: u64,
    pub granularity: u64,
    #[cfg(test)]
    pub free_override: Option<u64>,
}

impl Budget {
    pub fn new(path: &Path, limit: u64) -> Result<Arc<Mutex<Self>>, TorrentError> {
        let stats = fs4::statvfs(path).map_err(TorrentError::from)?;
        Ok(Arc::new(Mutex::new(Self {
            used: 0,
            limit,
            granularity: stats.allocation_granularity().max(1),
            #[cfg(test)]
            free_override: None,
        })))
    }

    pub fn block_charge(&self) -> u64 {
        BLOCK
            .div_ceil(self.granularity)
            .saturating_mul(self.granularity)
    }

    pub fn available(&self, path: &Path, bytes: u64) -> Result<(), TorrentError> {
        #[cfg(test)]
        let free = self
            .free_override
            .map(Ok)
            .unwrap_or_else(|| fs4::available_space(path));
        #[cfg(not(test))]
        let free = fs4::available_space(path);
        if free.map_err(TorrentError::from)? < bytes {
            return Err(TorrentError::Disk(DiskError::Full));
        }
        Ok(())
    }
}

struct Blocks {
    files: HashMap<(usize, u64), u64>,
    allocated: u64,
    retired: bool,
}

pub(super) struct Cache {
    pub path: PathBuf,
    pub budget: Arc<Mutex<Budget>>,
    blocks: Mutex<Blocks>,
    pub error: Mutex<Option<TorrentError>>,
    owner: OwnedDir,
}

impl Cache {
    #[cfg(test)]
    pub fn new(path: PathBuf, budget: Arc<Mutex<Budget>>) -> Result<Arc<Self>, TorrentError> {
        Ok(Self::in_directory(OwnedDir::create(&path)?, budget))
    }

    pub fn in_directory(owner: OwnedDir, budget: Arc<Mutex<Budget>>) -> Arc<Self> {
        Arc::new(Self {
            path: owner.path.clone(),
            owner,
            budget,
            blocks: Mutex::new(Blocks {
                files: HashMap::new(),
                allocated: 0,
                retired: false,
            }),
            error: Mutex::new(None),
        })
    }

    pub fn exhausted_for(&self, files: &[(usize, u64)]) -> bool {
        let blocks = lock(&self.blocks);
        let budget = lock(&self.budget);
        budget.limit.saturating_sub(budget.used) < budget.block_charge()
            && files.iter().any(|(index, size)| {
                let allocated = blocks
                    .files
                    .keys()
                    .filter(|(file, _)| file == index)
                    .count() as u64;
                allocated < size.div_ceil(BLOCK)
            })
    }

    pub fn retire(&self) -> Result<(), TorrentError> {
        let mut blocks = lock(&self.blocks);
        blocks.retired = true;
        blocks.files.clear();
        self.owner.remove()?;
        let mut budget = lock(&self.budget);
        budget.used = budget.used.saturating_sub(blocks.allocated);
        blocks.allocated = 0;
        Ok(())
    }

    fn read(&self, file: usize, mut offset: u64, mut buf: &mut [u8]) -> std::io::Result<()> {
        let blocks = lock(&self.blocks);
        while !buf.is_empty() {
            let key = (file, offset / BLOCK);
            if !blocks.files.contains_key(&key) {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
            }
            let mut chunk = self.owner.with(|dir| {
                dir.open(format!("{}-{}", key.0, key.1))
                    .map(|file| file.into_std())
            })?;
            let within = offset % BLOCK;
            let count = buf.len().min((BLOCK - within) as usize);
            chunk.seek(SeekFrom::Start(within))?;
            chunk.read_exact(&mut buf[..count])?;
            buf = &mut buf[count..];
            offset += count as u64;
        }
        Ok(())
    }

    fn write(&self, file: usize, mut offset: u64, mut buf: &[u8]) -> Result<(), TorrentError> {
        let mut blocks = lock(&self.blocks);
        let mut budget = lock(&self.budget);
        if blocks.retired || lock(&self.error).is_some() {
            return Err(TorrentError::Cancelled);
        }
        while !buf.is_empty() {
            let key = (file, offset / BLOCK);
            if let std::collections::hash_map::Entry::Vacant(entry) = blocks.files.entry(key) {
                let charge = budget.block_charge();
                if charge > budget.limit.saturating_sub(budget.used) {
                    return Err(TorrentError::Disk(DiskError::Full));
                }
                budget.available(&self.path, charge)?;
                let path = format!("{}-{}", key.0, key.1);
                let result = (|| {
                    let chunk = self.owner.with(|dir| {
                        dir.open_with(
                            &path,
                            cap_std::fs::OpenOptions::new()
                                .create_new(true)
                                .read(true)
                                .write(true),
                        )
                        .map(|file| file.into_std())
                    })?;
                    chunk.allocate(charge)?;
                    chunk.set_len(BLOCK)?;
                    let actual = chunk.allocated_size()?;
                    if actual != charge {
                        return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
                    }
                    Ok::<_, std::io::Error>(actual)
                })();
                let actual = match result {
                    Ok(result) => result,
                    Err(error) => {
                        let _ = self.owner.with(|dir| dir.remove_file(&path));
                        return Err(error.into());
                    }
                };
                budget.used += actual;
                entry.insert(actual);
                blocks.allocated += actual;
            }
            let within = offset % BLOCK;
            let count = buf.len().min((BLOCK - within) as usize);
            let mut chunk = self.owner.with(|dir| {
                dir.open_with(
                    format!("{}-{}", key.0, key.1),
                    cap_std::fs::OpenOptions::new().read(true).write(true),
                )
                .map(|file| file.into_std())
            })?;
            chunk.seek(SeekFrom::Start(within))?;
            chunk.write_all(&buf[..count])?;
            buf = &buf[count..];
            offset = offset
                .checked_add(count as u64)
                .ok_or(TorrentError::Engine)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct Factory(pub Arc<Cache>);
pub(super) struct Storage(Mutex<Option<Arc<Cache>>>);

impl Storage {
    fn cache(&self) -> anyhow::Result<Arc<Cache>> {
        lock(&self.0)
            .clone()
            .ok_or_else(|| anyhow::anyhow!("torrent storage retired"))
    }
}

impl StorageFactory for Factory {
    type Storage = Storage;
    fn create(&self, _: &ManagedTorrentShared, _: &TorrentMetadata) -> anyhow::Result<Storage> {
        Ok(Storage(Mutex::new(Some(self.0.clone()))))
    }
    fn clone_box(&self) -> librqbit::storage::BoxStorageFactory {
        self.clone().boxed()
    }
}

impl TorrentStorage for Storage {
    fn init(&mut self, _: &ManagedTorrentShared, _: &TorrentMetadata) -> anyhow::Result<()> {
        Ok(())
    }
    fn pread_exact(&self, file: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        self.cache()?.read(file, offset, buf)?;
        Ok(())
    }
    fn pwrite_all(&self, file: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        let cache = self.cache()?;
        let result = cache.write(file, offset, buf);
        if let Err(error) = result {
            lock(&cache.error).get_or_insert(error);
        }
        result.map_err(Into::into)
    }
    fn remove_file(&self, _: usize, _: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    fn remove_directory_if_empty(&self, _: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    fn ensure_file_length(&self, _: usize, _: u64) -> anyhow::Result<()> {
        Ok(())
    }
    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        let cache = lock(&self.0)
            .take()
            .ok_or_else(|| anyhow::anyhow!("torrent storage already taken"))?;
        Ok(Box::new(Self(Mutex::new(Some(cache)))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    #[test]
    fn allocated_cap_sparse_offsets_overwrites_and_retirement() {
        let dir = tempfile::tempdir().unwrap();
        let budget = Budget::new(dir.path(), BLOCK * 2).unwrap();
        let cache = Cache::new(dir.path().join("one"), budget.clone()).unwrap();
        cache.write(0, 1 << 40, &[1; 100]).unwrap();
        cache.write(0, 1 << 40, &[2; 100]).unwrap();
        let mut buf = [0; 100];
        cache.read(0, 1 << 40, &mut buf).unwrap();
        assert_eq!(buf, [2; 100]);
        cache.write(0, 0, &[3]).unwrap();
        assert_eq!(
            cache.write(0, BLOCK, &[4]),
            Err(TorrentError::Disk(DiskError::Full))
        );
        let actual: u64 = fs::read_dir(&cache.path)
            .unwrap()
            .map(|entry| {
                File::open(entry.unwrap().path())
                    .unwrap()
                    .allocated_size()
                    .unwrap()
            })
            .sum();
        assert_eq!(actual, lock(&budget).used);
        assert!(actual <= BLOCK * 2);
        cache.retire().unwrap();
        assert_eq!(lock(&budget).used, 0);
        assert!(cache.write(0, 0, &[1]).is_err());
    }

    #[test]
    fn free_space_failure_does_not_allocate() {
        let dir = tempfile::tempdir().unwrap();
        let budget = Budget::new(dir.path(), BLOCK).unwrap();
        lock(&budget).free_override = Some(0);
        let cache = Cache::new(dir.path().join("one"), budget.clone()).unwrap();
        assert_eq!(
            cache.write(0, 0, &[1]),
            Err(TorrentError::Disk(DiskError::Full))
        );
        assert_eq!(lock(&budget).used, 0);
        assert_eq!(fs::read_dir(&cache.path).unwrap().count(), 0);
    }
}
