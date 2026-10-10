//! Owner directories and disk accounting ported from `desktop/main/media-cache.ts`.

use crate::media::{MediaError, token};
use fs4::fs_std::FileExt;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(super) struct Disk {
    pub path: PathBuf,
    pub lock: Option<File>,
}

fn directory(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn owner_name(name: &str) -> bool {
    let mut parts = name.split('-');
    parts.next() == Some("owner")
        && parts
            .next()
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
        && parts.next().is_some_and(|random| {
            !random.is_empty()
                && random
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        && parts.next().is_some()
}

impl Disk {
    pub fn open(root: &Path) -> Result<Self, MediaError> {
        directory(root).map_err(|_| MediaError::Transport)?;
        for entry in fs::read_dir(root)
            .map_err(|_| MediaError::Transport)?
            .flatten()
        {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir())
                || !owner_name(&entry.file_name().to_string_lossy())
            {
                continue;
            }
            let lock_path = entry.path().join("owner.lock");
            if let Ok(file) = options().open(lock_path)
                && file.try_lock().is_ok()
            {
                drop(file);
                let _ = fs::remove_dir_all(entry.path());
            }
        }
        let path = root.join(format!(
            "owner-{}-{}-{}",
            std::process::id(),
            &token()?[..16],
            token()?
        ));
        #[cfg(unix)]
        let staging = root.join(format!("pending-{}", token()?));
        #[cfg(not(unix))]
        let staging = path.clone();
        directory(&staging).map_err(|_| MediaError::Transport)?;
        let result = (|| {
            let file = options()
                .create_new(true)
                .open(staging.join("owner.lock"))
                .map_err(|_| MediaError::Transport)?;
            file.try_lock().map_err(|_| MediaError::Transport)?;
            #[cfg(unix)]
            fs::rename(&staging, &path).map_err(|_| MediaError::Transport)?;
            Ok(Self {
                path: path.clone(),
                lock: Some(file),
            })
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(staging);
        }
        result
    }

    pub fn allocation(&self, bytes: u64, reserve: u64) -> Option<u64> {
        let stats = fs4::statvfs(&self.path).ok()?;
        let block = stats.allocation_granularity().max(1);
        let allocated = bytes
            .div_ceil(block)
            .checked_mul(block)?
            .checked_add(block)?;
        (stats.available_space().checked_sub(allocated)? >= reserve).then_some(allocated)
    }

    pub fn write(&self, key: &str, bytes: &[u8], allocated: u64) -> io::Result<(PathBuf, u64)> {
        let path = self
            .path
            .join(format!("{:x}", Sha256::digest(key.as_bytes())));
        let result = (|| {
            let mut file = options().create_new(true).open(&path)?;
            file.write_all(bytes)?;
            let actual = (file.allocated_size()?.saturating_add(4096)).max(allocated);
            Ok((path.clone(), actual))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&path);
        }
        result
    }
}

impl Drop for Disk {
    fn drop(&mut self) {
        self.lock.take();
        let _ = fs::remove_dir_all(&self.path);
    }
}
