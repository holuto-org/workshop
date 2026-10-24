use std::{fs, path::Path};

use tempfile::TempDir;

use super::{StateDir, write_durably};
use crate::Error;

#[test]
fn a_second_instance_is_refused_until_the_first_is_gone() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("state");
    let first = StateDir::open(path.clone()).unwrap();
    assert!(
        matches!(StateDir::open(path.clone()), Err(Error::Blocked(message)) if message.contains("już działa"))
    );
    drop(first);
    let _second = StateDir::open(path).unwrap();
}

#[test]
fn a_durable_write_replaces_the_whole_file() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("file.json");
    write_durably(&path, b"first, longer content").unwrap();
    write_durably(&path, b"second").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second");
    let leftovers = fs::read_dir(temporary.path()).unwrap().count();
    assert_eq!(leftovers, 1, "no temporary file stays behind");
}

#[cfg(unix)]
#[test]
fn state_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = TempDir::new().unwrap();
    let dir = StateDir::open(temporary.path().join("state")).unwrap();
    let file = dir.file("settings.json");
    write_durably(&file, b"{}").unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(dir.path()), 0o700);
    assert_eq!(mode(&file), 0o600);
}

/// An overridden `%LOCALAPPDATA%` must not move the state directory, or the app would open a fresh
/// journal and lock beside the real ones and slip past an unresolved operation. The override is
/// set on a child run of this test binary: the crate forbids the `unsafe` `set_var`.
#[cfg(windows)]
#[test]
fn the_windows_state_directory_ignores_an_overridden_localappdata() {
    use std::{env, process::Command};

    const EXPECTED: &str = "HOLUTO_TEST_EXPECTED_STATE_DIR";
    if let Some(expected) = env::var_os(EXPECTED) {
        assert_eq!(StateDir::default_path().unwrap(), Path::new(&expected));
        return;
    }
    let child = Command::new(env::current_exe().unwrap())
        .args([
            "store::tests::the_windows_state_directory_ignores_an_overridden_localappdata",
            "--exact",
        ])
        .env(EXPECTED, StateDir::default_path().unwrap())
        .env("LOCALAPPDATA", r"C:\overridden")
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&child.stdout);
    // An unmatched filter also exits successfully, having checked nothing.
    assert!(
        child.status.success() && report.contains("1 passed"),
        "the child saw a different state directory:\n{report}"
    );
}
