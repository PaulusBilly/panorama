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
}

struct Directory {
    parent: Dir,
    name: OsString,
    dir: Dir,
    identity: Handle,
}

impl OwnedDir {
    pub fn create(path: &Path) -> Result<Self, TorrentError> {
        let path = std::path::absolute(path)?;
        let parent = Dir::open_ambient_dir(
            path.parent().ok_or(TorrentError::InvalidSource)?,
            cap_std::ambient_authority(),
        )?;
        Self::create_in(parent, path)
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
        let directory = Directory::open(parent, name)?;
        directory.verify()?;
        Ok(Self {
            path,
            inner: Mutex::new(Some(directory)),
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
