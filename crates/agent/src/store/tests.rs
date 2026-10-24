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
