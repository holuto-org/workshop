#[cfg(not(windows))]
use std::fs;
#[cfg(target_os = "linux")]
use std::io;
use std::{error::Error, path::Path};

#[cfg(not(windows))]
use super::set_at;

type TestResult = Result<(), Box<dyn Error>>;

#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove artifacts; setup propagates I/O"
)]
fn linux_login_entry_launches_only_the_exact_executable_and_is_removable() -> TestResult {
    let temp = tempfile::tempdir()?;
    let artifact = temp.path().join("autostart/workshop.desktop");
    set_at(
        &artifact,
        Path::new("/home/operator/Holuto Workshop/workshop"),
        true,
    )?;
    assert_eq!(
        fs::read_to_string(&artifact)?,
        "[Desktop Entry]\nType=Application\nName=Holuto Workshop\n\
         Exec=\"/home/operator/Holuto Workshop/workshop\"\n\
         Terminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    set_at(&artifact, Path::new("/unused"), false)?;
    assert!(!artifact.exists());
    set_at(&artifact, Path::new("/unused"), false)?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove artifacts; setup propagates I/O"
)]
fn linux_login_entry_after_a_package_upgrade_launches_the_installed_executable() -> TestResult {
    let temp = tempfile::tempdir()?;
    let artifact = temp.path().join("autostart/workshop.desktop");
    set_at(&artifact, Path::new("/usr/bin/workshop (deleted)"), true)?;
    assert_eq!(
        fs::read_to_string(&artifact)?,
        "[Desktop Entry]\nType=Application\nName=Holuto Workshop\n\
         Exec=\"/usr/bin/workshop\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove artifacts; setup propagates I/O"
)]
fn desktop_exec_launches_paths_with_field_codes_and_shell_metacharacters() -> TestResult {
    let temp = tempfile::tempdir()?;
    let artifact = temp.path().join("workshop.desktop");
    for path in ["/tmp/a%u$`\"\\workshop", "/opt/\\\\x/$HOME/%%/workshop"] {
        set_at(&artifact, Path::new(path), true)?;
        let entry = fs::read_to_string(&artifact)?;
        let exec = entry
            .lines()
            .find_map(|line| line.strip_prefix("Exec="))
            .ok_or("missing Exec key")?;
        assert_eq!(exec_path(exec)?, path, "{exec}");
    }
    let before = fs::read(&artifact)?;
    let error = set_at(&artifact, Path::new("/tmp/work\tshop"), true).unwrap_err();
    assert!(
        matches!(&error, agent::Error::Store { source, .. }
            if source.kind() == io::ErrorKind::InvalidInput),
        "an unusable executable path is a local failure: {error}"
    );
    assert_eq!(
        error.to_string(),
        "Nie można zmienić autostartu (/tmp/work\tshop): \
         Ścieżka aplikacji nie nadaje się do autostartu."
    );
    assert_eq!(
        fs::read(&artifact)?,
        before,
        "refusal preserves the existing registration"
    );
    Ok(())
}

/// Independent decoder for one quoted Exec argument; rejects unsupported escapes and field codes.
#[cfg(target_os = "linux")]
fn exec_path(value: &str) -> Result<String, Box<dyn Error>> {
    let mut string = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' && characters.next() != Some('\\') {
            return Err("unexpected string escape".into());
        }
        string.push(character);
    }
    let inner = string
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or("not one quoted argument")?;
    let mut path = String::new();
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some(escaped @ ('"' | '`' | '$' | '\\')) => path.push(escaped),
                other => return Err(format!("invalid Exec escape {other:?}").into()),
            },
            '"' | '`' | '$' => return Err(format!("unescaped {character} in quotes").into()),
            '%' => {
                if characters.next() != Some('%') {
                    return Err("field code".into());
                }
                path.push('%');
            },
            other => path.push(other),
        }
    }
    Ok(path)
}

#[cfg(target_os = "macos")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove artifacts; setup propagates I/O"
)]
fn macos_login_agent_uses_exact_executable_and_is_removable() -> TestResult {
    let temp = tempfile::tempdir()?;
    let artifact = temp
        .path()
        .join("Library/LaunchAgents/com.holuto.workshop.plist");
    set_at(
        &artifact,
        Path::new("/Users/operator/Holuto & Workshop.app/Contents/MacOS/workshop"),
        true,
    )?;
    assert_eq!(
        fs::read_to_string(&artifact)?,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict><key>Label</key><string>com.holuto.workshop</string>\
         <key>ProgramArguments</key><array>\
         <string>/Users/operator/Holuto &amp; Workshop.app/Contents/MacOS/workshop</string>\
         </array><key>RunAtLoad</key><true/></dict></plist>\n"
    );
    set_at(&artifact, Path::new("/unused"), false)?;
    assert!(!artifact.exists());
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The registration is made by a child launched through a symlink in an isolated HOME"
)]
fn macos_login_agent_launched_through_the_cli_symlink_names_the_bundle_executable() -> TestResult {
    use std::{env, os::unix::fs::symlink, process::Command};
    const CHILD: &str = "WORKSHOP_AUTOSTART_SYMLINK_CHILD";
    if env::var_os(CHILD).is_some() {
        return Ok(super::set_enabled(true)?);
    }
    let home = tempfile::tempdir()?;
    let executable = fs::canonicalize(env::current_exe()?)?;
    let launcher = home.path().join(".local/bin/workshop");
    fs::create_dir_all(launcher.parent().ok_or("missing launcher directory")?)?;
    symlink(&executable, &launcher)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "Never change the operator's autostart"
    )]
    let output = Command::new(&launcher)
        .args([
            "--exact",
            concat!(
                "autostart::tests::",
                "macos_login_agent_launched_through_the_cli_symlink_names_the_bundle_executable"
            ),
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("HOME", home.path())
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(
            home.path()
                .join("Library/LaunchAgents/com.holuto.workshop.plist")
        )?,
        super::registration(executable.to_str().ok_or("non-UTF-8 test executable")?),
        "the login item must not depend on the launcher symlink"
    );
    Ok(())
}

#[cfg(windows)]
#[test]
#[expect(
    unsafe_code,
    clippy::panic_in_result_fn,
    reason = "Read and remove only this test's disposable HKCU key with checked buffers"
)]
fn windows_login_value_contains_only_the_quoted_executable_and_is_removable() -> TestResult {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegDeleteKeyW, RegDeleteTreeW,
            RegGetValueW, RegOpenKeyExW,
        },
    };
    struct Cleanup(Vec<u16>);
    impl Cleanup {
        fn remove(&self) -> (WIN32_ERROR, WIN32_ERROR) {
            let parent: Vec<_> = "Software\\HolutoWorkshopTests"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: live NUL-terminated names below this test's own HKCU subtree;
            // deleting a parent that still has subkeys fails and leaves it intact.
            unsafe {
                (
                    RegDeleteTreeW(HKEY_CURRENT_USER, self.0.as_ptr()),
                    RegDeleteKeyW(HKEY_CURRENT_USER, parent.as_ptr()),
                )
            }
        }
    }
    impl Drop for Cleanup {
        // Best effort after a failed assertion; the normal path checks `remove` itself.
        fn drop(&mut self) {
            let _results = self.remove();
        }
    }
    let temp = tempfile::tempdir()?;
    let key = format!(
        "Software\\HolutoWorkshopTests\\{}",
        temp.path()
            .file_name()
            .ok_or("temp directory has no name")?
            .to_string_lossy()
    );
    let executable = Path::new("C:\\Users\\Operator\\Holuto Workshop\\workshop.exe");
    let key_wide: Vec<_> = key.encode_utf16().chain(Some(0)).collect();
    let cleanup = Cleanup(key_wide.clone());
    super::registry(&key, executable, true)?;
    let name: Vec<_> = "Holuto Workshop".encode_utf16().chain(Some(0)).collect();
    let expected: Vec<_> = "\"C:\\Users\\Operator\\Holuto Workshop\\workshop.exe\""
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut bytes = vec![0_u8; expected.len()];
    let mut size = u32::try_from(bytes.len())?;
    // SAFETY: live terminated strings, writable byte buffer and byte count match.
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            bytes.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    assert_eq!(found, ERROR_SUCCESS);
    assert_eq!(bytes, expected);
    super::registry(&key, executable, false)?;
    // SAFETY: same live buffers as above; missing value leaves the buffer untouched.
    let absent = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            bytes.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    assert_eq!(absent, ERROR_FILE_NOT_FOUND);
    let (leaf, parent) = cleanup.remove();
    assert_eq!(leaf, ERROR_SUCCESS, "this test's key is deleted");
    assert!(
        // Another run's key keeps the parent (access denied) or already removed it.
        matches!(
            parent,
            ERROR_SUCCESS | ERROR_ACCESS_DENIED | ERROR_FILE_NOT_FOUND
        ),
        "unexpected parent deletion result {parent}"
    );
    let mut handle = ptr::null_mut();
    // SAFETY: live NUL-terminated name; output is a local HKEY slot closed if opened.
    let reopened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            0,
            KEY_READ,
            &raw mut handle,
        )
    };
    if reopened == ERROR_SUCCESS {
        // SAFETY: successful open returned this owned handle, closed exactly once.
        let _closed = unsafe { RegCloseKey(handle) };
    }
    assert_eq!(reopened, ERROR_FILE_NOT_FOUND, "no disposable key remains");
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Autostart rollback and path selection are exercised in an isolated child HOME"
)]
fn failed_preference_save_restores_registration_and_relative_xdg_uses_home() -> TestResult {
    use agent::{Error as AgentError, settings::Settings, store::StateDir};
    use std::{env, process::Command};
    const CHILD: &str = "WORKSHOP_AUTOSTART_ROLLBACK_CHILD";
    if let Some(home) = env::var_os(CHILD) {
        let home = Path::new(&home);
        let dir = StateDir::open(home.join("state"))?;
        fs::create_dir(dir.path().join("settings.json"))?;
        let artifact = home.join(".config/autostart/workshop.desktop");
        for previous in [false, true] {
            super::set_enabled(previous)?;
            let before = fs::read(&artifact).ok();
            let error = super::configure(
                &dir,
                Settings {
                    start_on_launch: previous,
                    ..Settings::default()
                },
                !previous,
            )
            .unwrap_err();
            assert!(
                matches!(error, AgentError::Store { action: "Nie można zapisać ustawień", path, .. }
                if path == dir.path().join("settings.json"))
            );
            assert_eq!(
                fs::read(&artifact).ok(),
                before,
                "failed save restores the previous opt-in"
            );
            assert!(dir.path().join("settings.json").is_dir());
        }
        super::set_enabled(false)?;
        fs::write(home.join("outcome"), "rolled back off and on; removed\n")?;
        return Ok(());
    }
    for xdg in [None, Some("relative-config"), Some("")] {
        let temporary = tempfile::tempdir()?;
        #[expect(
            clippy::disallowed_methods,
            reason = "Never change the operator's autostart"
        )]
        let mut child = Command::new(env::current_exe()?);
        let _command = child
            .args([
                "--exact",
                concat!(
                    "autostart::tests::",
                    "failed_preference_save_restores_registration_and_relative_xdg_uses_home"
                ),
                "--nocapture",
            ])
            .env(CHILD, temporary.path())
            .env("HOME", temporary.path());
        if let Some(xdg) = xdg {
            let _command = child.env("XDG_CONFIG_HOME", xdg);
        } else {
            let _command = child.env_remove("XDG_CONFIG_HOME");
        }
        let output = child.output()?;
        assert!(output.status.success(), "{xdg:?}: {output:?}");
        assert_eq!(
            fs::read_to_string(temporary.path().join("outcome"))?,
            "rolled back off and on; removed\n"
        );
        assert!(
            !temporary
                .path()
                .join(".config/autostart/workshop.desktop")
                .exists()
        );
    }
    Ok(())
}

#[cfg(not(windows))]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Only a missing registration is an idempotent removal"
)]
fn removing_a_registration_directory_reports_the_store_error_without_removing_it() -> TestResult {
    use agent::Error as AgentError;
    let temporary = tempfile::tempdir()?;
    let artifact = temporary.path().join("workshop.desktop");
    fs::create_dir(&artifact)?;
    let error = set_at(&artifact, Path::new("/unused"), false).unwrap_err();
    assert!(matches!(error, AgentError::Store {
        action: "Nie można zmienić autostartu", path, ..
    } if path == artifact));
    assert!(artifact.is_dir());
    Ok(())
}
