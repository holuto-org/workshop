#[cfg(not(windows))]
use std::env;
use std::{
    fs::{self, File, TryLockError},
    io::{self, Write as _},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use atomicwrites::{AtomicFile, OverwriteBehavior};

use crate::Error;

const LOCK: &str = "instance.lock";
/// Held only by the running application: CLI commands take `LOCK` too.
const APPLICATION_LOCK: &str = "application.lock";
/// Status readers hold their shared lock for microseconds; this only bounds a
/// pathological reader, never a running application.
const READER_WAIT: Duration = Duration::from_secs(1);

/// Exclusive state-directory lock prevents concurrent printer or journal access.
#[derive(Debug)]
pub struct StateDir {
    path: PathBuf,
    _lock: File,
    /// `Some` only when opened as the running application.
    _application: Option<File>,
}

impl StateDir {
    /// Rejects empty or relative base paths to avoid silently moving the journal.
    pub fn default_path() -> Option<PathBuf> {
        Self::default_path_for(false)
    }

    /// Simulator state is isolated from the production fiscal journal.
    pub fn default_path_for(simulator: bool) -> Option<PathBuf> {
        base_dir().map(|base| {
            base.join(if simulator {
                "workshop-simulator"
            } else {
                "workshop"
            })
        })
    }

    /// Opens the default directory as the running application.
    pub fn open_default(simulator: bool) -> Result<Self, Error> {
        let path = Self::default_path_for(simulator).ok_or_else(|| {
            Error::Invalid("Nie można ustalić katalogu stanu użytkownika.".to_owned())
        })?;
        Self::open_application(path)
    }

    /// Whether the application, not a CLI command, holds the directory.
    ///
    /// Checks an existing lock without creating a directory or lock file. Takes only a
    /// shared lock, so a starting application can tell a reader from another instance.
    pub fn is_application_running(path: &Path) -> Result<bool, Error> {
        let lock_path = path.join(APPLICATION_LOCK);
        let lock = match File::options().read(true).write(true).open(&lock_path) {
            Ok(lock) => lock,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(source) => {
                return Err(Error::Store {
                    action: "Nie można odczytać blokady katalogu stanu",
                    path: lock_path,
                    source,
                });
            },
        };
        match lock.try_lock_shared() {
            Ok(()) => Ok(false),
            Err(TryLockError::WouldBlock) => Ok(true),
            Err(TryLockError::Error(source)) => Err(Error::Store {
                action: "Nie można sprawdzić blokady katalogu stanu",
                path: lock_path,
                source,
            }),
        }
    }

    pub fn open(path: PathBuf) -> Result<Self, Error> {
        Self::lock(path, false)
    }

    /// Like `open`, and marks the holder as the application for `is_application_running`.
    pub fn open_application(path: PathBuf) -> Result<Self, Error> {
        Self::lock(path, true)
    }

    fn lock(path: PathBuf, application: bool) -> Result<Self, Error> {
        create_private_dir(&path).map_err(|source| Error::Store {
            action: "Nie można utworzyć katalogu stanu",
            path: path.clone(),
            source,
        })?;
        let lock = exclusive(&path, LOCK)?;
        // Taken under the instance lock, so only status readers can contend for it.
        let application = application
            .then(|| exclusive(&path, APPLICATION_LOCK))
            .transpose()?;
        Ok(Self {
            path,
            _lock: lock,
            _application: application,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

/// Opens and exclusively locks `directory/name`, refusing while another process holds it.
fn exclusive(directory: &Path, name: &str) -> Result<File, Error> {
    let lock_path = directory.join(name);
    // Windows cannot lock an append-only handle.
    let mut options = File::options();
    let _options = options.read(true).create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        let _options = options.mode(0o600);
    }
    let lock = options.open(&lock_path).map_err(|source| Error::Store {
        action: "Nie można otworzyć blokady katalogu stanu",
        path: lock_path.clone(),
        source,
    })?;
    #[cfg(unix)]
    make_private(&lock).map_err(|source| Error::Store {
        action: "Nie można zabezpieczyć blokady katalogu stanu",
        path: lock_path.clone(),
        source,
    })?;
    match acquire(&lock) {
        Ok(true) => Ok(lock),
        // CLI commands take the instance lock too; name the holder the operator must wait for.
        Ok(false)
            if name == LOCK && !StateDir::is_application_running(directory).unwrap_or(true) =>
        {
            Err(Error::Busy(format!(
                "Inne polecenie workshop używa katalogu {}. Spróbuj ponownie, gdy się zakończy.",
                directory.display()
            )))
        },
        Ok(false) => Err(Error::Blocked(format!(
            "Holuto Workshop już działa dla katalogu {}. Otwórz je z zasobnika systemowego.",
            directory.display()
        ))),
        Err(source) => Err(Error::Store {
            action: "Nie można zablokować katalogu stanu",
            path: lock_path,
            source,
        }),
    }
}

/// Takes an exclusive lock; `Ok(false)` means another process holds it exclusively.
///
/// Exclusive contention is either another holder (exclusive) or a status reader
/// (shared, `is_application_running`). Only the latter is waited out, so a starting
/// application never mistakes `workshop status` for a second instance.
fn acquire(lock: &File) -> io::Result<bool> {
    let deadline = Instant::now() + READER_WAIT;
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(true),
            Err(TryLockError::WouldBlock) => {},
            Err(TryLockError::Error(error)) => return Err(error),
        }
        match lock.try_lock_shared() {
            // Only readers hold it; release before the next exclusive attempt.
            Ok(()) => lock.unlock()?,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(error)) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::yield_now();
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
    use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// Tightens existing files too: creation mode alone does not protect upgrades.
#[cfg(unix)]
pub(crate) fn make_private(file: &File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

/// Atomically replaces content through a private sibling temporary file (0600 on Unix).
///
/// Unix fsyncs the file and its directory. Crashes leave old or new content; failed
/// temporary writes are removed on drop.
#[cfg(not(windows))]
pub(crate) fn write_durably(path: &Path, bytes: &[u8]) -> io::Result<()> {
    AtomicFile::new(path, OverwriteBehavior::AllowOverwrite)
        .write_with_options(|file| file.write_all(bytes), private_new_file())
        .map_err(io::Error::from)
}

/// Atomically replaces content through a synced sibling temporary file.
///
/// `fs::rename` uses POSIX rename semantics on NTFS, so the replacement succeeds while
/// another process (a status reader, an antivirus scan) has the target open; legacy
/// `MoveFileExW` replacement fails with access denied then. Flushing the renamed handle
/// commits the rename to the NTFS log. Crashes leave old or new content; a failed
/// replacement removes its temporary file.
#[cfg(windows)]
pub(crate) fn write_durably(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::{
        process,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?
        .to_owned();
    name.push(format!(
        ".{}.{}.tmp",
        process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let temporary = path.with_file_name(name);
    let mut file = private_new_file().open(&temporary)?;
    let replaced = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = replaced {
        drop(file);
        let _removed = fs::remove_file(&temporary);
        return Err(error);
    }
    file.sync_all()
}

fn private_new_file() -> fs::OpenOptions {
    let mut options = File::options();
    _ = options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        let _options = options.mode(0o600);
    }
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
