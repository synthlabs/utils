use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::{Component, Path, PathBuf},
};

pub(crate) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) struct FileLock(File);

impl FileLock {
    pub(crate) fn try_lock(file: File, exclusive: bool) -> Result<Self, TryLockError> {
        if exclusive {
            file.try_lock()?;
        } else {
            file.try_lock_shared()?;
        }
        Ok(Self(file))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // A duplicated or inherited handle may outlive this guard.
        let _ = self.0.unlock();
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Root(PathBuf);

impl Root {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        fs::create_dir_all(path)?;
        let path = path.canonicalize()?;
        if !path.is_dir() {
            return Err(invalid("store root is not a directory"));
        }
        Ok(Self(path))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// The caller-selected root may resolve a link; descendants may not.
    pub(crate) fn checked(&self, relative: &Path) -> io::Result<PathBuf> {
        let mut path = self.0.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(invalid("managed paths must contain only normal components"));
            };
            path.push(name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if is_link(&metadata) || !path.canonicalize()?.starts_with(&self.0) {
                        return Err(invalid("link or path outside the managed root"));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(path)
    }

    pub(crate) fn directory(&self, relative: &Path) -> io::Result<PathBuf> {
        let mut current = PathBuf::new();
        for component in relative.components() {
            if !matches!(component, Component::Normal(_)) {
                return Err(invalid("invalid managed directory"));
            }
            current.push(component);
            let path = self.checked(&current)?;
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            if !fs::symlink_metadata(&path)?.is_dir() {
                return Err(invalid("managed directory is not a directory"));
            }
        }
        self.checked(relative)
    }

    pub(crate) fn lock_file(&self, name: &str) -> io::Result<File> {
        self.directory(Path::new(".locks"))?;
        let path = self.checked(&Path::new(".locks").join(name))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(invalid("lock is not a regular file"));
        }
        Ok(file)
    }

    pub(crate) fn read(&self, relative: &Path, limit: usize) -> io::Result<Vec<u8>> {
        use io::Read;
        let file = File::open(self.checked(relative)?)?;
        if !file.metadata()?.is_file() {
            return Err(invalid("not a regular file"));
        }
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(invalid("managed metadata exceeds its size limit"));
        }
        Ok(bytes)
    }

    pub(crate) fn atomic_write(&self, relative: &Path, bytes: &[u8]) -> io::Result<()> {
        use io::Write;
        let target = self.checked(relative)?;
        let parent = target.parent().ok_or_else(|| invalid("missing parent"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        self.checked(relative)?;
        temporary.persist(target).map_err(|error| error.error)?;
        Ok(())
    }
}

pub(crate) fn check_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link(&metadata) {
        return Err(invalid("refusing a link in a managed installation"));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            check_tree(&entry?.path())?;
        }
    } else if !metadata.is_file() {
        return Err(invalid("refusing a non-regular installation file"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_lock_owner_releases_lock_with_duplicate_handle_open() {
        for exclusive in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = Root::open(directory.path()).unwrap();
            let file = root.lock_file("retained.lock").unwrap();
            let duplicate = file.try_clone().unwrap();
            let lock = FileLock::try_lock(file, exclusive).unwrap();
            let contender = root.lock_file("retained.lock").unwrap();
            assert!(matches!(
                contender.try_lock(),
                Err(TryLockError::WouldBlock)
            ));
            drop(lock);
            contender
                .try_lock()
                .expect("dropping the owner must release its lock while a duplicate remains open");
            drop(duplicate);
        }
    }

    #[test]
    fn dropping_one_reader_preserves_other_readers_and_failed_writer_attempts() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path()).unwrap();
        let first = FileLock::try_lock(root.lock_file("readers.lock").unwrap(), false).unwrap();
        let second = FileLock::try_lock(root.lock_file("readers.lock").unwrap(), false).unwrap();
        drop(first);
        for _ in 0..2 {
            assert!(matches!(
                FileLock::try_lock(root.lock_file("readers.lock").unwrap(), true),
                Err(TryLockError::WouldBlock)
            ));
        }
        drop(second);
        FileLock::try_lock(root.lock_file("readers.lock").unwrap(), true).unwrap();
    }
}
