//! Per-user login registration for the CLI and background settings writer.

use std::{env, io, path::Path};
#[cfg(not(windows))]
use std::{fs, io::Write as _, path::PathBuf};
#[cfg(windows)]
use std::{iter, os::windows::ffi::OsStrExt as _, ptr};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
    },
};

use agent::{Error, settings::Settings, store::StateDir};
#[cfg(not(windows))]
use atomicwrites::{AtomicFile, OverwriteBehavior};

pub(crate) fn configure(
    dir: &StateDir,
    mut settings: Settings,
    enabled: bool,
) -> Result<(), Error> {
    let previous = settings.start_on_launch;
    settings.start_on_launch = enabled;
    settings.validate_configuration()?;
    set_enabled(enabled)?;
    if let Err(error) = settings.save(dir) {
        set_enabled(previous)?;
        return Err(error);
    }
    Ok(())
}

pub(crate) fn set_enabled(enabled: bool) -> Result<(), Error> {
    let executable =
        env::current_exe().map_err(|source| store_error(Path::new("workshop"), source))?;
    // macOS reports the path as invoked: through the `~/.local/bin` launcher symlink the login
    // item would launch that link, not the bundle. Removal must work even once it is gone.
    #[cfg(target_os = "macos")]
    let executable = if enabled {
        fs::canonicalize(&executable).map_err(|source| store_error(&executable, source))?
    } else {
        executable
    };
    #[cfg(windows)]
    {
        registry(RUN, &executable, enabled)?;
        // Task Manager's "Disable" is a separate StartupApproved entry that outranks Run;
        // removing it lets Windows apply its default (enabled) to the value just written.
        registry(STARTUP_APPROVED, &executable, false)
    }
    #[cfg(not(windows))]
    {
        set_at(&artifact_path()?, &executable, enabled)
    }
}

#[cfg(windows)]
const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(windows)]
const STARTUP_APPROVED: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";

#[cfg(not(windows))]
fn artifact_path() -> Result<PathBuf, Error> {
    let home = || {
        env::home_dir()
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                local_error(
                    Path::new("~"),
                    io::ErrorKind::NotFound,
                    "Nie można ustalić katalogu domowego dla autostartu.",
                )
            })
    };
    #[cfg(target_os = "macos")]
    {
        Ok(home()?.join("Library/LaunchAgents/com.holuto.workshop.plist"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let config = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        Ok(config
            .map_or_else(|| home().map(|path| path.join(".config")), Ok)?
            .join("autostart/workshop.desktop"))
    }
}

fn store_error(path: &Path, source: io::Error) -> Error {
    Error::Store {
        action: "Nie można zmienić autostartu",
        path: path.to_owned(),
        source,
    }
}

/// The environment, not the saved configuration, prevents registration.
fn local_error(path: &Path, kind: io::ErrorKind, reason: &str) -> Error {
    store_error(path, io::Error::new(kind, reason))
}

#[cfg(not(windows))]
fn set_at(artifact: &Path, executable: &Path, enabled: bool) -> Result<(), Error> {
    if !enabled {
        return match fs::remove_file(artifact) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(store_error(artifact, error)),
        };
    }
    let text = executable
        .to_str()
        // Desktop-entry strings and plist XML both forbid raw control characters.
        .filter(|text| !text.chars().any(char::is_control))
        .ok_or_else(|| {
            local_error(
                executable,
                io::ErrorKind::InvalidInput,
                "Ścieżka aplikacji nie nadaje się do autostartu.",
            )
        })?;
    // `/proc/self/exe` marks a binary a package upgrade renamed over the running one;
    // the installed path is the one to launch at the next login.
    #[cfg(target_os = "linux")]
    let text = text.strip_suffix(" (deleted)").unwrap_or(text);
    let content = registration(text);
    if let Some(parent) = artifact.parent() {
        fs::create_dir_all(parent).map_err(|source| store_error(artifact, source))?;
    }
    AtomicFile::new(artifact, OverwriteBehavior::AllowOverwrite)
        .write(|file| file.write_all(content.as_bytes()))
        .map_err(|error| store_error(artifact, io::Error::from(error)))
}

/// Exec has two escape layers: desktop-entry strings, then quoted arguments.
///
/// Double each argument-escape backslash for the string layer:
/// <https://specifications.freedesktop.org/desktop-entry-spec/latest/exec-variables.html>.
#[cfg(all(not(windows), not(target_os = "macos")))]
fn registration(executable: &str) -> String {
    let mut escaped = String::with_capacity(executable.len());
    for character in executable.chars() {
        match character {
            '%' => escaped.push_str("%%"),
            '\\' => escaped.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                escaped.push_str("\\\\");
                escaped.push(character);
            },
            _ => escaped.push(character),
        }
    }
    format!(
        "[Desktop Entry]\nType=Application\nName=Holuto Workshop\n\
         Exec=\"{escaped}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(target_os = "macos")]
fn registration(executable: &str) -> String {
    let escaped = executable
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict><key>Label</key><string>com.holuto.workshop</string>\
         <key>ProgramArguments</key><array><string>{escaped}</string></array>\
         <key>RunAtLoad</key><true/></dict></plist>\n"
    )
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Win32 registry calls contain raw buffers and close every acquired handle"
)]
fn registry(key: &str, executable: &Path, enabled: bool) -> Result<(), Error> {
    let key: Vec<_> = key.encode_utf16().chain(Some(0)).collect();
    let name: Vec<_> = "Holuto Workshop".encode_utf16().chain(Some(0)).collect();
    let mut handle = ptr::null_mut();
    // SAFETY: NUL-terminated names remain alive; output is a local valid HKEY slot.
    let opened = unsafe {
        if enabled {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                ptr::null(),
                &raw mut handle,
                ptr::null_mut(),
            )
        } else {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                KEY_SET_VALUE,
                &raw mut handle,
            )
        }
    };
    if !enabled && opened == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if opened != ERROR_SUCCESS {
        return Err(registry_error(executable, opened));
    }
    let changed = if enabled {
        let quoted: Vec<_> = iter::once(u16::from(b'"'))
            .chain(executable.as_os_str().encode_wide())
            .chain([u16::from(b'"'), 0])
            .collect();
        if quoted.len() > 261 {
            // SAFETY: successful open returned this owned handle, closed exactly once.
            let _closed = unsafe { RegCloseKey(handle) };
            return Err(local_error(
                executable,
                io::ErrorKind::InvalidInput,
                "Ścieżka autostartu przekracza limit 260 znaków Windows.",
            ));
        }
        let bytes: Vec<_> = quoted.into_iter().flat_map(u16::to_le_bytes).collect();
        let count = u32::try_from(bytes.len()).map_err(|error| {
            local_error(executable, io::ErrorKind::InvalidInput, &error.to_string())
        })?;
        // SAFETY: live byte buffer includes the UTF-16 NUL; byte length is checked.
        unsafe { RegSetValueExW(handle, name.as_ptr(), 0, REG_SZ, bytes.as_ptr(), count) }
    } else {
        // SAFETY: valid open handle and live NUL-terminated value name.
        unsafe { RegDeleteValueW(handle, name.as_ptr()) }
    };
    // SAFETY: successful open returned this owned handle, closed exactly once.
    let closed = unsafe { RegCloseKey(handle) };
    if changed != ERROR_SUCCESS && (enabled || changed != ERROR_FILE_NOT_FOUND) {
        return Err(registry_error(executable, changed));
    }
    if closed != ERROR_SUCCESS {
        return Err(registry_error(executable, closed));
    }
    Ok(())
}

#[cfg(windows)]
fn registry_error(path: &Path, code: u32) -> Error {
    store_error(
        path,
        io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX)),
    )
}

#[cfg(test)]
#[path = "autostart/tests.rs"]
mod tests;
