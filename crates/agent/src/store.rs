#[cfg(not(windows))]
use std::env;
use std::{
    fs::{self, File, TryLockError},
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use atomicwrites::{AtomicFile, OverwriteBehavior};

use crate::Error;

const LOCK: &str = "instance.lock";

/// Exclusive state-directory lock prevents concurrent printer or journal access.
#[derive(Debug)]
pub struct StateDir {
    path: PathBuf,
    /// Retained until drop; process exit also releases the lock.
    _lock: File,
}

impl StateDir {
    /// Linux: `$XDG_STATE_HOME/holuto-fiscal` or `~/.local/state/holuto-fiscal`.
    /// macOS: `~/Library/Application Support/holuto-fiscal`.
    /// Windows: `holuto-fiscal` in the Local AppData Known Folder.
    /// Ignores empty or relative base paths to avoid silently moving the journal.
    pub fn default_path() -> Option<PathBuf> {
        base_dir().map(|base| base.join("holuto-fiscal"))
    }

    pub fn open(path: PathBuf) -> Result<Self, Error> {
        create_private_dir(&path).map_err(|source| Error::Store {
            action: "Nie można utworzyć katalogu stanu",
            path: path.clone(),
            source,
        })?;
        let lock_path = path.join(LOCK);
        // Windows cannot lock an append-only handle.
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

/// Environment overrides must not open a fresh journal beside an unresolved one.
#[cfg(windows)]
fn base_dir() -> Option<PathBuf> {
    known_folders::get_known_folder_path(known_folders::KnownFolder::LocalAppData)
}

#[cfg(target_os = "macos")]
fn base_dir() -> Option<PathBuf> {
    Some(absolute(env::home_dir()?)?.join("Library/Application Support"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn base_dir() -> Option<PathBuf> {
    env::var_os("XDG_STATE_HOME")
        .and_then(absolute)
        .or_else(|| Some(absolute(env::home_dir()?)?.join(".local/state")))
}

#[cfg(not(windows))]
fn absolute(path: impl Into<PathBuf>) -> Option<PathBuf> {
    let path = path.into();
    path.is_absolute().then_some(path)
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

/// Syncs a temporary file (0600 on Unix) in a private sibling directory, then renames it.
/// Durability uses directory fsync on Unix and `MoveFileExW(MOVEFILE_WRITE_THROUGH)` on Windows.
/// Crashes leave old or new content, never a torn file; drop removes failed temporary writes.
pub(crate) fn write_durably(path: &Path, bytes: &[u8]) -> io::Result<()> {
    AtomicFile::new(path, OverwriteBehavior::AllowOverwrite)
        .write_with_options(|file| file.write_all(bytes), private_new_file())
        .map_err(io::Error::from)
}

#[cfg(unix)]
fn private_new_file() -> fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut options = File::options();
    _ = options.write(true).create_new(true).mode(0o600);
    options
}

#[cfg(not(unix))]
fn private_new_file() -> fs::OpenOptions {
    let mut options = File::options();
    _ = options.write(true).create_new(true);
    options
}

pub(crate) fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
