use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::StoreError;

pub(super) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

pub(super) fn parent(path: &Path) -> &Path {
    match path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        Some(parent) => parent,
        None => Path::new("."),
    }
}

/// Takes the exclusive per-database lock file (`<db>.lock`), held for the store's lifetime.
/// One holder at a time means no other connection exists while this process opens the
/// database through raw file handles (which on Unix would drop SQLite's POSIX locks) or
/// quarantines it, so a healthy replacement can never be quarantined by a stale opener.
pub(super) fn lock(path: &Path) -> Result<fs::File, StoreError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(parent(path))?;
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(sibling(path, ".lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(fs::TryLockError::WouldBlock) => Err(StoreError::Locked),
        Err(fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

/// Restricts the database's existing sidecars and quarantined copies to the owner (Unix).
/// SQLite derives new sidecar modes from the database file, so this runs before it opens.
pub(super) fn restrict_siblings(path: &Path) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        for entry in fs::read_dir(parent(path))? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let ours = name
                .as_deref()
                .is_some_and(|name| file_name.starts_with(name) && file_name != name);
            if ours && entry.file_type()?.is_file() {
                fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o600))?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(super) fn prepare(path: &Path) -> Result<bool, StoreError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(parent(path))?;
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let (file, created) = match options.open(path) {
        Ok(file) => (file, true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (
            fs::OpenOptions::new().read(true).write(true).open(path)?,
            false,
        ),
        Err(error) => return Err(error.into()),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    drop(file);
    Ok(created)
}

pub(super) fn quarantine(path: &Path, now_ms: i64) -> Result<PathBuf, StoreError> {
    let mut timestamp = now_ms;
    let destination = loop {
        let candidate = sibling(path, &format!(".corrupt-{timestamp}"));
        if !candidate.try_exists()?
            && !sibling(&candidate, "-wal").try_exists()?
            && !sibling(&candidate, "-shm").try_exists()?
        {
            break candidate;
        }
        timestamp = timestamp.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "quarantine timestamp exhausted",
            )
        })?;
    };
    let mut moved = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let source = sibling(path, suffix);
        let target = sibling(&destination, suffix);
        if suffix.is_empty() || source.try_exists()? {
            if let Err(error) = fs::rename(&source, &target) {
                for (source, target) in moved.iter().rev() {
                    fs::rename(target, source)?;
                }
                return Err(error.into());
            }
            moved.push((source, target));
        }
    }
    retain_newest(path, 2)?;
    Ok(destination)
}

/// Deletes all but the `keep` newest quarantined copies (and their sidecars) of `path`.
pub(super) fn retain_newest(path: &Path, keep: usize) -> Result<(), StoreError> {
    let mut quarantines = Vec::new();
    for entry in fs::read_dir(parent(path))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if let Some((_, suffix)) = text.rsplit_once(".corrupt-")
            && let Ok(timestamp) = suffix.parse::<i64>()
            && sibling(path, &format!(".corrupt-{timestamp}")).file_name() == Some(name.as_os_str())
        {
            quarantines.push((timestamp, entry.path()));
        }
    }
    quarantines.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in quarantines.into_iter().skip(keep) {
        for suffix in ["-wal", "-shm", ""] {
            match fs::remove_file(sibling(&path, suffix)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}
