use super::{DiskError, TorrentError, lock};
use cap_std::fs::{Dir, Metadata};
use same_file::Handle;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub(super) struct OwnedDir {
    pub path: PathBuf,
    inner: Mutex<Option<Directory>>,
    root: Mutex<Option<Directory>>,
}

struct Directory {
    parent: Dir,
    name: OsString,
    dir: Dir,
    identity: Handle,
}

impl OwnedDir {
    #[cfg(test)]
    pub fn create(path: &Path) -> Result<Self, TorrentError> {
        let path = std::path::absolute(path)?;
        let parent = Dir::open_ambient_dir(
            path.parent().ok_or(TorrentError::InvalidSource)?,
            cap_std::ambient_authority(),
        )?;
        Self::create_in(parent, path)
    }

    /// Treats `path` as a persistent root: creates or opens it without following
    /// links, sweeps stale token-named children left by dead launches, and
    /// creates a fresh exclusive per-launch child that this value owns.
    pub fn create_session(path: &Path) -> Result<Self, TorrentError> {
        let path = std::path::absolute(path)?;
        let name = path
            .file_name()
            .ok_or(TorrentError::InvalidSource)?
            .to_owned();
        let parent = Dir::open_ambient_dir(
            path.parent().ok_or(TorrentError::InvalidSource)?,
            cap_std::ambient_authority(),
        )?;
        match parent.create_dir(&name) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let root = Directory::open(parent, name)?;
        root.verify()?;
        root.sweep();
        let mut attempts = 0;
        loop {
            let token = session_token()?;
            match root.dir.create_dir(&token) {
                Ok(()) => {
                    let mut owned = Self::open_created(root.dir.try_clone()?, path.join(&token))?;
                    owned.root = Mutex::new(Some(root));
                    return Ok(owned);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempts < 8 => {
                    attempts += 1;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub fn child(&self, name: &str) -> Result<Self, TorrentError> {
        let inner = lock(&self.inner);
        let directory = inner.as_ref().ok_or(TorrentError::Cancelled)?;
        directory.verify()?;
        Self::create_in(directory.dir.try_clone()?, self.path.join(name))
    }

    fn create_in(parent: Dir, path: PathBuf) -> Result<Self, TorrentError> {
        let name = path
            .file_name()
            .ok_or(TorrentError::InvalidSource)?
            .to_owned();
        parent.create_dir(&name)?;
        Self::open_created(parent, path)
    }

    fn open_created(parent: Dir, path: PathBuf) -> Result<Self, TorrentError> {
        let name = path
            .file_name()
            .ok_or(TorrentError::InvalidSource)?
            .to_owned();
        let directory = Directory::open(parent, name)?;
        directory.verify()?;
        Ok(Self {
            path,
            inner: Mutex::new(Some(directory)),
            root: Mutex::new(None),
        })
    }

    pub fn with<T>(
        &self,
        operation: impl FnOnce(&Dir) -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        let inner = lock(&self.inner);
        let directory = inner
            .as_ref()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        operation(&directory.dir)
    }

    pub fn remove(&self) -> Result<(), TorrentError> {
        self.remove_owned()?;
        if let Some(root) = lock(&self.root).take() {
            // Best effort: another launch may still own a child of the root.
            let Directory {
                parent,
                name,
                dir,
                identity,
            } = root;
            drop(identity);
            drop(dir);
            let _ = parent.remove_dir(name);
        }
        Ok(())
    }

    fn remove_owned(&self) -> Result<(), TorrentError> {
        let mut inner = lock(&self.inner);
        let Some(directory) = inner.as_ref() else {
            return Ok(());
        };
        directory.verify()?;
        directory.clear()?;
        directory.verify()?;
        let directory = inner.take().ok_or(TorrentError::Engine)?;
        let Directory {
            parent,
            name,
            dir,
            identity,
        } = directory;
        drop(identity);
        drop(dir);
        parent.remove_dir(name)?;
        Ok(())
    }
}

impl Directory {
    fn open(parent: Dir, name: OsString) -> std::io::Result<Self> {
        reject_link(&parent.symlink_metadata(&name)?)?;
        let parent_file = parent.try_clone()?.into_std_file();
        let file = cap_primitives::fs::open_dir_nofollow(&parent_file, Path::new(&name))?;
        let identity = Handle::from_file(file.try_clone()?)?;
        let dir = Dir::from_std_file(file);
        reject_link(&dir.dir_metadata()?)?;
        Ok(Self {
            parent,
            name,
            dir,
            identity,
        })
    }

    fn verify(&self) -> std::io::Result<()> {
        reject_link(&self.parent.symlink_metadata(&self.name)?)?;
        let parent = self.parent.try_clone()?.into_std_file();
        let file = cap_primitives::fs::open_dir_nofollow(&parent, Path::new(&self.name))?;
        if self.identity != Handle::from_file(file)? {
            return Err(std::io::Error::other("cache directory identity changed"));
        }
        Ok(())
    }

    /// Best-effort removal of stale token-named children. Never follows links
    /// and ignores every failure; directories held open elsewhere survive.
    fn sweep(&self) {
        let Ok(entries) = self.dir.entries() else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_str().is_some_and(is_session_token) {
                continue;
            }
            let Ok(metadata) = self.dir.symlink_metadata(&name) else {
                continue;
            };
            if reject_link(&metadata).is_err() || !metadata.is_dir() {
                continue;
            }
            let Ok(clone) = self.dir.try_clone() else {
                continue;
            };
            let Ok(child) = Self::open(clone, name) else {
                continue;
            };
            if child.clear().is_err() || child.verify().is_err() {
                continue;
            }
            let Self {
                parent,
                name,
                dir,
                identity,
            } = child;
            drop(identity);
            drop(dir);
            let _ = parent.remove_dir(name);
        }
    }

    fn clear(&self) -> std::io::Result<()> {
        self.verify()?;
        for entry in self.dir.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            let metadata = self.dir.symlink_metadata(&name)?;
            reject_link(&metadata)?;
            if metadata.is_dir() {
                let child = Self::open(self.dir.try_clone()?, name)?;
                child.clear()?;
                child.verify()?;
                let Self {
                    parent,
                    name,
                    dir,
                    identity,
                } = child;
                drop(identity);
                drop(dir);
                parent.remove_dir(name)?;
            } else {
                self.dir.remove_file(name)?;
            }
        }
        Ok(())
    }
}

fn reject_link(metadata: &Metadata) -> std::io::Result<()> {
    #[cfg(windows)]
    let reparse = {
        use cap_std::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.is_symlink() || reparse {
        return Err(std::io::Error::other(TorrentError::Disk(DiskError::Io)));
    }
    Ok(())
}

fn session_token() -> Result<String, TorrentError> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).map_err(|_| TorrentError::Engine)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn is_session_token(name: &str) -> bool {
    name.len() == 16
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leftover(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::create_dir_all(path.join("nested")).unwrap();
        std::fs::write(path.join("junk"), b"junk").unwrap();
        std::fs::write(path.join("nested").join("junk"), b"junk").unwrap();
        path
    }

    #[test]
    fn creates_root_and_removes_child_then_empty_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("cache");
        let owned = OwnedDir::create_session(&root).unwrap();
        assert_eq!(owned.path.parent().unwrap(), root);
        assert!(is_session_token(
            owned.path.file_name().unwrap().to_str().unwrap()
        ));
        assert!(owned.path.is_dir());
        owned.remove().unwrap();
        assert!(!owned.path.exists());
        assert!(!root.exists());
    }

    #[test]
    fn sweeps_token_children_and_keeps_other_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("cache");
        let stale = leftover(&root, "0123456789abcdef");
        let upper = leftover(&root, "ABCDEF0123456789");
        let other = leftover(&root, "keep-me");
        let file = root.join("fedcba9876543210");
        std::fs::write(&file, b"file").unwrap();
        let owned = OwnedDir::create_session(&root).unwrap();
        assert!(!stale.exists());
        assert!(upper.exists() && other.join("junk").exists() && file.exists());
        owned.remove().unwrap();
        assert!(!owned.path.exists());
        assert!(root.exists(), "root with foreign entries is kept");
    }

    #[test]
    fn concurrent_sessions_get_distinct_children() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("cache");
        let first = OwnedDir::create_session(&root).unwrap();
        let second = OwnedDir::create_session(&root).unwrap();
        assert_ne!(first.path, second.path);
        assert!(first.path.is_dir());
        first.remove().unwrap();
        second.remove().unwrap();
        assert!(!root.exists());
    }

    #[test]
    fn linked_root_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("external");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("sentinel"), b"keep").unwrap();
        let stale = leftover(&target, "0123456789abcdef");
        let root = dir.path().join("cache");
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd.exe")
                .args(["/c", "mklink", "/J"])
                .arg(&root)
                .arg(&target)
                .output()
                .unwrap()
                .status
                .success()
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &root).unwrap();
        assert!(OwnedDir::create_session(&root).is_err());
        assert_eq!(std::fs::read(target.join("sentinel")).unwrap(), b"keep");
        assert!(stale.join("junk").exists());
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 2);
    }
}
