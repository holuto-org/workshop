//! The state directory: settings, the operation journal and the receipt history, owned by one
//! running instance at a time.

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

/// The state directory, locked for this process: a second instance (or `--probe` while the app
/// runs) is refused, so two processes can never drive the printer or the journal at once.
#[derive(Debug)]
pub struct StateDir {
    path: PathBuf,
    /// Held for the lifetime of the value; the OS releases it when the process exits.
    _lock: File,
}

impl StateDir {
    /// `$XDG_STATE_HOME/holuto-fiscal` (default `~/.local/state/holuto-fiscal`) on Linux,
    /// `~/Library/Application Support/holuto-fiscal` on macOS and `%LOCALAPPDATA%\holuto-fiscal`
    /// on Windows. Relative or empty values are ignored rather than resolved against the working
    /// directory, which would silently move the journal.
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

/// The Local AppData Known Folder, not the `%LOCALAPPDATA%` variable: a process environment may
/// override it, and following the override would open a fresh journal beside the real one.
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

/// Replaces `path` atomically and durably.
///
/// The bytes are synced in a temporary file (mode 0600 on Unix) inside a private subdirectory
/// next to the target, then renamed over it durably: a directory fsync on Unix,
/// `MoveFileExW(MOVEFILE_WRITE_THROUGH)` on Windows. A crash or power loss leaves either the old
/// or the new content, never a torn file; on failure the temporary directory is deleted on drop.
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
