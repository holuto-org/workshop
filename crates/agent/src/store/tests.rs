#[cfg(target_os = "linux")]
use std::ffi::OsStr;
use std::{
    env,
    error::Error as StdError,
    fs::{self, File},
    io::{self, BufRead as _, BufReader, Read as _, Write as _},
    path::PathBuf,
    process::{Command, Stdio},
};

use super::{StateDir, write_durably};
use crate::Error;

type TestResult = Result<(), Box<dyn StdError>>;

fn child(test: &str) -> Result<Command, Box<dyn StdError>> {
    #[expect(
        clippy::disallowed_methods,
        reason = "State-path and lock tests isolate environment in the current test binary."
    )]
    let mut command = Command::new(env::current_exe()?);
    let _command = command.args([test, "--exact", "--nocapture"]);
    Ok(command)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A completed child leaves evidence independent of the test runner output."
)]
fn assert_child(command: &mut Command) -> TestResult {
    let temporary = tempfile::tempdir()?;
    let outcome = temporary.path().join("outcome");
    let output = command
        .env("HOLUTO_TEST_CHILD_OUTCOME", &outcome)
        .output()?;
    if !output.status.success() {
        return Err(format!("child failed: {output:?}").into());
    }
    assert_eq!(fs::read(outcome)?, b"complete");
    Ok(())
}

fn child_succeeded() -> TestResult {
    fs::write(
        env::var_os("HOLUTO_TEST_CHILD_OUTCOME").ok_or("missing outcome path")?,
        b"complete",
    )?;
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn another_process_cannot_open_state_until_the_holder_releases_it() -> TestResult {
    const PATH: &str = "HOLUTO_TEST_LOCK_PATH";
    const BLOCKED: &str = "HOLUTO_TEST_LOCK_BLOCKED";
    if let Some(path) = env::var_os(PATH) {
        let path = PathBuf::from(path);
        if env::var_os(BLOCKED).is_some() {
            // The holder is another command, not the application.
            let expected = format!(
                "Inne polecenie workshop używa katalogu {}. Spróbuj ponownie, gdy się zakończy.",
                path.display()
            );
            assert!(
                matches!(StateDir::open(path), Err(Error::Busy(message)) if message.to_string() == expected),
                "a competing process must receive the exact lock refusal"
            );
        } else {
            let _dir = StateDir::open(path)?;
        }
        return child_succeeded();
    }
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("Zażółć gęślą jaźń");
    let dir = StateDir::open(path.clone())?;
    let mut blocked =
        child("store::tests::another_process_cannot_open_state_until_the_holder_releases_it")?;
    let _command = blocked.env(PATH, &path).env(BLOCKED, "1");
    assert_child(&mut blocked)?;
    drop(dir);
    let mut released =
        child("store::tests::another_process_cannot_open_state_until_the_holder_releases_it")?;
    let _command = released.env(PATH, &path).env_remove(BLOCKED);
    assert_child(&mut released)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn atomic_replacement_keeps_an_open_reader_on_the_previous_complete_file() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("Zażółć – ustawienia.json");
    write_durably(&path, b"first, longer content")?;
    let mut reader = File::open(&path)?;
    write_durably(&path, b"second")?;
    let mut previous = Vec::new();
    #[expect(
        clippy::verbose_file_reads,
        reason = "This test must read the already-open old handle, not reopen the replaced path."
    )]
    let _bytes = reader.read_to_end(&mut previous)?;
    assert_eq!(
        previous, b"first, longer content",
        "an already-open reader never sees torn replacement content"
    );
    assert_eq!(
        fs::read(&path)?,
        b"second",
        "new readers see the complete replacement"
    );
    let names = fs::read_dir(temporary.path())?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        names,
        [path.file_name().ok_or("missing filename")?.to_os_string()],
        "successful replacement leaves no staging files"
    );
    Ok(())
}

#[cfg(windows)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Stale temporary files must not block a new write or lose crash evidence."
)]
fn stale_windows_temporaries_do_not_prevent_durable_replacement() -> TestResult {
    const PATH: &str = "HOLUTO_TEST_STALE_TEMPORARY_PATH";
    if let Some(path) = env::var_os(PATH) {
        let path = PathBuf::from(path);
        fs::write(&path, b"previous complete content")?;
        let mut stale = Vec::new();
        for counter in 0..2 {
            let mut name = path.file_name().ok_or("missing filename")?.to_owned();
            name.push(format!(".{}.{counter}.tmp", std::process::id()));
            let temporary = path.with_file_name(name);
            fs::write(&temporary, b"retained crash evidence")?;
            stale.push(temporary);
        }
        // The fresh child has not written durably yet: its first candidate is counter zero.
        write_durably(&path, b"replacement complete content")?;
        assert_eq!(fs::read(&path)?, b"replacement complete content");
        for temporary in stale {
            assert_eq!(fs::read(temporary)?, b"retained crash evidence");
        }
        assert_eq!(
            fs::read_dir(path.parent().ok_or("missing parent")?)?.count(),
            3,
            "the successful write removes only its own temporary name"
        );
        return child_succeeded();
    }
    let temporary = tempfile::tempdir()?;
    let mut command =
        child("store::tests::stale_windows_temporaries_do_not_prevent_durable_replacement")?;
    let _command = command.env(PATH, temporary.path().join("Ustawienia – Łódź.json"));
    assert_child(&mut command)
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn existing_state_and_lock_are_tightened_before_use() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("state");
    fs::create_dir(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    fs::write(path.join("instance.lock"), b"")?;
    fs::set_permissions(
        path.join("instance.lock"),
        fs::Permissions::from_mode(0o644),
    )?;
    let dir = StateDir::open(path)?;
    write_durably(&dir.file("settings.json"), b"{}")?;
    assert_eq!(
        fs::metadata(dir.path())?.permissions().mode() & 0o777,
        0o700,
        "existing state must become private too"
    );
    assert_eq!(
        fs::metadata(dir.file("instance.lock"))?
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "existing instance lock must become private"
    );
    assert_eq!(
        fs::metadata(dir.file("settings.json"))?
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "atomic files remain private"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn newly_created_state_and_locks_are_private() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open_application(temporary.path().join("nested/stan"))?;
    assert_eq!(
        fs::metadata(dir.path())?.permissions().mode() & 0o777,
        0o700,
        "new state directory is owner-only"
    );
    for lock in ["instance.lock", "application.lock"] {
        assert_eq!(
            fs::metadata(dir.file(lock))?.permissions().mode() & 0o777,
            0o600,
            "new {lock} is owner-only regardless of umask"
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn linux_uses_absolute_xdg_state_or_falls_back_to_home() -> TestResult {
    const EXPECTED: &str = "HOLUTO_TEST_EXPECTED_STATE_DIR";
    if let Some(expected) = env::var_os(EXPECTED) {
        assert_eq!(
            StateDir::default_path(),
            Some(PathBuf::from(expected)),
            "Linux must not open a second journal through relative overrides"
        );
        return child_succeeded();
    }
    let temporary = tempfile::tempdir()?;
    let home = temporary.path().join("Dom – Łódź");
    let xdg = temporary.path().join("Stan – Żółć");
    for (override_value, expected) in [
        (Some(xdg.as_os_str()), xdg.join("workshop")),
        (
            Some(OsStr::new("relative")),
            home.join(".local/state/workshop"),
        ),
        (Some(OsStr::new("")), home.join(".local/state/workshop")),
        (None, home.join(".local/state/workshop")),
    ] {
        let mut command =
            child("store::tests::linux_uses_absolute_xdg_state_or_falls_back_to_home")?;
        let _command = command.env("HOME", &home).env(EXPECTED, expected);
        if let Some(value) = override_value {
            let _command = command.env("XDG_STATE_HOME", value);
        } else {
            let _command = command.env_remove("XDG_STATE_HOME");
        }
        assert_child(&mut command)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn macos_uses_home_application_support_and_ignores_xdg() -> TestResult {
    const EXPECTED: &str = "HOLUTO_TEST_EXPECTED_STATE_DIR";
    if let Some(expected) = env::var_os(EXPECTED) {
        assert_eq!(
            StateDir::default_path(),
            Some(PathBuf::from(expected)),
            "macOS state belongs in Application Support"
        );
        return child_succeeded();
    }
    let temporary = tempfile::tempdir()?;
    let home = temporary.path().join("Dom – Łódź");
    let mut command = child("store::tests::macos_uses_home_application_support_and_ignores_xdg")?;
    let _command = command
        .env("HOME", &home)
        .env("XDG_STATE_HOME", temporary.path().join("ignored"))
        .env(EXPECTED, home.join("Library/Application Support/workshop"));
    assert_child(&mut command)
}

#[cfg(windows)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn windows_uses_the_known_folder_and_ignores_localappdata() -> TestResult {
    const EXPECTED: &str = "HOLUTO_TEST_EXPECTED_STATE_DIR";
    if let Some(expected) = env::var_os(EXPECTED) {
        assert_eq!(
            StateDir::default_path(),
            Some(PathBuf::from(expected)),
            "environment overrides cannot move the Windows fiscal journal"
        );
        return child_succeeded();
    }
    let expected = known_folders::get_known_folder_path(known_folders::KnownFolder::LocalAppData)
        .ok_or("LocalAppData Known Folder unavailable")?
        .join("workshop");
    let temporary = tempfile::tempdir()?;
    let mut command =
        child("store::tests::windows_uses_the_known_folder_and_ignores_localappdata")?;
    let _command = command
        .env("LOCALAPPDATA", temporary.path().join("Fałszywy katalog"))
        .env(EXPECTED, expected);
    assert_child(&mut command)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn a_process_exit_releases_its_instance_lock() -> TestResult {
    const PATH: &str = "HOLUTO_TEST_EXIT_LOCK_PATH";
    if let Some(path) = env::var_os(PATH) {
        let _dir = StateDir::open(PathBuf::from(path))?;
        let mut stdout = io::stdout().lock();
        stdout.write_all(b"HOLUTO_LOCK_HELD\n")?;
        stdout.flush()?;
        let mut byte = [0];
        let _read = io::stdin().read(&mut byte)?;
        return Ok(());
    }
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("Stan – Łódź");
    let mut command = child("store::tests::a_process_exit_releases_its_instance_lock")?;
    let _command = command
        .env(PATH, &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut holder = command.spawn()?;
    let stdout = holder.stdout.take().ok_or("child stdout unavailable")?;
    let mut output = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        if output.read_line(&mut line)? == 0 {
            let _status = holder.wait()?;
            return Err("lock holder exited before its readiness signal".into());
        }
        if line.contains("HOLUTO_LOCK_HELD") {
            break;
        }
        line.clear();
    }
    let blocked = StateDir::open(path.clone());
    holder.kill()?;
    let status = holder.wait()?;
    assert!(
        !status.success(),
        "the holder must exit abruptly, without dropping StateDir normally"
    );
    assert!(
        matches!(blocked, Err(Error::Busy(_))),
        "a lock held by another process must exclude this process"
    );
    let _recovered = StateDir::open(path)?;
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn a_second_open_in_the_same_process_cannot_bypass_the_instance_lock() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("state");
    let dir = StateDir::open_application(path.clone())?;
    assert!(StateDir::is_application_running(&path)?);
    let expected = format!(
        "Holuto Workshop już działa dla katalogu {}. Otwórz je z zasobnika systemowego.",
        path.display()
    );
    assert!(
        matches!(StateDir::open(path.clone()), Err(Error::Blocked(message)) if message.to_string() == expected),
        "opening a second handle must not bypass the lock"
    );
    drop(dir);
    assert!(!StateDir::is_application_running(&path)?);
    let _reopened = StateDir::open(path)?;
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn lock_status_does_not_create_missing_state_or_lock() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("absent");
    assert!(!StateDir::is_application_running(&path)?);
    assert!(!path.exists());
    fs::create_dir(&path)?;
    assert!(!StateDir::is_application_running(&path)?);
    assert!(!path.join("application.lock").exists());
    Ok(())
}

#[test]
fn concurrent_lock_status_checks_never_block_application_start() -> TestResult {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
    };

    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("state");
    drop(StateDir::open_application(path.clone())?);
    let done = Arc::new(AtomicBool::new(false));
    let checker = {
        let (done, path) = (Arc::clone(&done), path.clone());
        thread::Builder::new().name("status reader".into()).spawn(
            move || -> Result<(), String> {
                while !done.load(Ordering::SeqCst) {
                    let _running = StateDir::is_application_running(&path)
                        .map_err(|error| error.to_string())?;
                }
                Ok(())
            },
        )?
    };
    // `workshop status` polling while the app starts must not refuse the start.
    let opened = (0..2_000).try_for_each(|_| StateDir::open_application(path.clone()).map(drop));
    done.store(true, Ordering::SeqCst);
    let reader = checker.join().map_err(|_panic| "status reader panicked")?;
    opened?;
    reader?;
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn simulator_and_production_default_paths_are_distinct() -> TestResult {
    let production = StateDir::default_path().ok_or("no default state directory")?;
    assert_eq!(StateDir::default_path_for(false), Some(production.clone()));
    assert_eq!(
        StateDir::default_path_for(true),
        Some(production.with_file_name("workshop-simulator"))
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "An unreadable application lock is not a stopped application."
)]
fn unreadable_application_lock_is_an_error_instead_of_a_false_stopped_status() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let path = dir.file("application.lock");
    fs::create_dir(&path)?;
    let error = StateDir::is_application_running(dir.path())
        .err()
        .ok_or("unreadable lock ignored")?;
    let Error::Store {
        action,
        path: failed_path,
        ..
    } = error
    else {
        return Err("unreadable lock must report a store error".into());
    };
    assert_eq!(
        action.to_string(),
        "Nie można odczytać blokady katalogu stanu"
    );
    assert_eq!(failed_path, path);
    assert!(path.is_dir());
    Ok(())
}
