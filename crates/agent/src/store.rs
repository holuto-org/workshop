//! The state directory: settings, the operation journal and the receipt history, owned by one
//! running instance at a time.

use std::{
    fs::{self, File, TryLockError},
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use crate::Error;

const LOCK: &str = "instance.lock";

/// The state directory, locked for this process: a second instance (or `--probe` while the app
/// runs) is refused, so two processes can never drive the printer or the journal at once.
#[derive(Debug)]
pub struct StateDir {
    path: PathBuf,
    /// Held for the lifetime of the value; the OS releases it when the process exits.
    _lock: File,
}

impl StateDir {
    /// `$XDG_STATE_HOME/holuto-fiscal` on Linux, `~/Library/Application Support/holuto-fiscal`
    /// on macOS and `%LOCALAPPDATA%\holuto-fiscal` on Windows.
    pub fn default_path() -> Option<PathBuf> {
        dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .map(|base| base.join("holuto-fiscal"))
    }

    pub fn open(path: PathBuf) -> Result<Self, Error> {
        create_private_dir(&path).map_err(|source| Error::Store {
            action: "Nie można utworzyć katalogu stanu",
            path: path.clone(),
            source,
        })?;
        let lock_path = path.join(LOCK);
        // Read access: Windows cannot lock a handle opened for append only.
        let lock = File::options()
            .read(true)
            .create(true)
            .append(true)
            .open(&lock_path)
            .map_err(|source| Error::Store {
                action: "Nie można otworzyć blokady katalogu stanu",
                path: lock_path.clone(),
                source,
            })?;
        match lock.try_lock() {
            Ok(()) => Ok(Self { path, _lock: lock }),
            Err(TryLockError::WouldBlock) => Err(Error::Blocked(format!(
                "Holuto Fiscal już działa dla katalogu {}. Otwórz je z zasobnika systemowego.",
                path.display()
            ))),
            Err(TryLockError::Error(source)) => Err(Error::Store {
                action: "Nie można zablokować katalogu stanu",
                path: lock_path,
                source,
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// Replaces `path` atomically and durably.
///
/// The bytes are synced in a temporary file in the same directory (mode 0600 on Unix), then
/// renamed over the target durably (directory fsync on Unix, `MOVEFILE_WRITE_THROUGH` on
/// Windows), so a crash or power loss leaves either the old or the new content, never a torn file.
pub(crate) fn write_durably(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("ścieżka bez katalogu"))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".tmp-")
        .tempfile_in(directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    // On failure the temporary path is deleted on drop; after success nothing is left to delete.
    atomicwrites::replace_atomic(&temporary.into_temp_path(), path)
}

/// The file's bytes, or `None` when it does not exist.
pub(crate) fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
