use std::{
    error::Error as StdError,
    fs::{self, File},
    io::{ErrorKind, Read as _},
};

use super::Snapshot;
use crate::{Error, store::StateDir};

type TestResult = Result<(), Box<dyn StdError>>;

const GOLDEN: &str = r#"{
  "processing": true,
  "connected": true,
  "serial": "TEMO-1234",
  "printerId": "printer-5678",
  "blocked": true,
  "reportUnresolved": false,
  "orphanedReceipt": true,
  "updatedAt": "2026-10-24T12:34:56Z"
}"#;

fn snapshot() -> Result<Snapshot, jiff::Error> {
    Ok(Snapshot {
        processing: true,
        connected: true,
        serial: Some("TEMO-1234".to_owned()),
        printer_id: Some("printer-5678".to_owned()),
        blocked: true,
        report_unresolved: false,
        orphaned_receipt: true,
        updated_at: "2026-10-24T12:34:56Z".parse()?,
    })
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn saved_snapshot_matches_the_public_shape_and_roundtrips() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("Zażółć gęślą jaźń"))?;
    let expected = snapshot()?;
    expected.save(&dir)?;
    assert_eq!(fs::read(dir.file("status.json"))?, GOLDEN.as_bytes());
    assert_eq!(Snapshot::load_at(dir.path())?, Some(expected.clone()));
    assert_eq!(serde_json::from_str::<Snapshot>(GOLDEN)?, expected);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn missing_snapshot_is_absent_without_creating_state() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("state");
    assert_eq!(Snapshot::load_at(&path)?, None);
    assert!(!path.exists());
    let dir = StateDir::open(path)?;
    assert_eq!(Snapshot::load_at(dir.path())?, None);
    assert!(!dir.file("status.json").exists());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn disconnected_snapshot_retains_identity_and_blocks_without_inventing_processing() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let mut stopped = snapshot()?;
    stopped.processing = false;
    stopped.connected = false;
    stopped.report_unresolved = true;
    stopped.save(&dir)?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(dir.file("status.json"))?)?,
        serde_json::json!({
            "processing": false,
            "connected": false,
            "serial": "TEMO-1234",
            "printerId": "printer-5678",
            "blocked": true,
            "reportUnresolved": true,
            "orphanedReceipt": true,
            "updatedAt": "2026-10-24T12:34:56Z"
        })
    );
    stopped.serial = None;
    stopped.printer_id = None;
    stopped.save(&dir)?;
    let stored = serde_json::from_slice::<serde_json::Value>(&fs::read(dir.file("status.json"))?)?;
    assert_eq!(stored["serial"], serde_json::Value::Null);
    assert_eq!(stored["printerId"], serde_json::Value::Null);
    assert_eq!(Snapshot::load_at(dir.path())?, Some(stopped));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn malformed_snapshot_errors_never_echo_values_or_unknown_keys() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    for bytes in [
        "{".to_owned(),
        GOLDEN.replace("\"processing\": true", "\"processing\": null"),
        GOLDEN.replace("  \"processing\": true,\n", ""),
        GOLDEN.replace("\"processing\": true", "\"processing\": \"secret-token\""),
        GOLDEN.replace(
            "\"serial\": \"TEMO-1234\"",
            "\"serial\": {\"customer\": \"private-name\"}",
        ),
        GOLDEN.replace(
            "\"2026-10-24T12:34:56Z\"",
            "\"https://private.invalid/token\"",
        ),
        GOLDEN.replace("{\n", "{\n  \"secret-token\": \"customer-payload\",\n"),
    ] {
        fs::write(dir.file("status.json"), bytes.as_bytes())?;
        let error = Snapshot::load_at(dir.path())
            .err()
            .ok_or("malformed snapshot unexpectedly loaded")?;
        let Error::Store {
            action,
            path,
            source,
        } = &error
        else {
            return Err("malformed snapshot must report a store error".into());
        };
        assert_eq!(*action, "Nie można odczytać stanu aplikacji");
        assert_eq!(*path, dir.file("status.json"));
        assert_eq!(source.kind(), ErrorKind::InvalidData);
        assert_eq!(source.to_string(), "Nieprawidłowy JSON stanu aplikacji.");
        assert_eq!(
            error.to_string(),
            format!(
                "Nie można odczytać stanu aplikacji ({}): Nieprawidłowy JSON stanu aplikacji.",
                dir.file("status.json").display()
            )
        );
        assert_eq!(format!("{error:?}"), "Store { kind: InvalidData, .. }");
        assert_eq!(fs::read(dir.file("status.json"))?, bytes.as_bytes());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn replacement_keeps_existing_readers_on_the_previous_complete_snapshot() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let mut current = snapshot()?;
    current.save(&dir)?;
    let mut previous_reader = File::open(dir.file("status.json"))?;
    current.processing = false;
    current.connected = false;
    current.updated_at = "2026-10-24T12:35:00Z".parse()?;
    current.save(&dir)?;
    let mut previous = String::new();
    #[expect(
        clippy::verbose_file_reads,
        reason = "Read the already-open old handle rather than reopening the replaced path."
    )]
    let _bytes = previous_reader.read_to_string(&mut previous)?;
    assert_eq!(previous, GOLDEN);
    assert_eq!(Snapshot::load_at(dir.path())?, Some(current));
    assert_eq!(fs::read_dir(dir.path())?.count(), 2);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn refused_replacement_preserves_previous_snapshot_evidence() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    // A nonempty destination directory refuses replacement on every OS, even as root.
    let previous_dir = StateDir::open(dir.file("status.json"))?;
    let previous = snapshot()?;
    previous.save(&previous_dir)?;
    let mut replacement = previous.clone();
    replacement.processing = false;
    replacement.connected = false;
    let error = replacement
        .save(&dir)
        .err()
        .ok_or("replacement unexpectedly succeeded")?;
    let Error::Store { action, path, .. } = error else {
        return Err("refused replacement must report a store error".into());
    };
    assert_eq!(action, "Nie można zapisać stanu aplikacji");
    assert_eq!(path, dir.file("status.json"));
    assert_eq!(Snapshot::load_at(previous_dir.path())?, Some(previous));
    assert_eq!(
        fs::read(previous_dir.file("status.json"))?,
        GOLDEN.as_bytes()
    );
    assert_eq!(fs::read_dir(dir.path())?.count(), 2);
    Ok(())
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn new_and_replaced_snapshots_are_owner_only() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let current = snapshot()?;
    current.save(&dir)?;
    let path = dir.file("status.json");
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
    current.save(&dir)?;
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(Snapshot::load_at(dir.path())?, Some(current));
    Ok(())
}
