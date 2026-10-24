use std::{error::Error as StdError, fs};

use posnet::{money::Grosze, receipt::PaymentForm};
use uuid::Uuid;

use super::{HistoryEntry, append, load};
use crate::{Error, store::StateDir};

type TestResult = Result<(), Box<dyn StdError>>;

fn entry(id: u128) -> Result<HistoryEntry, Box<dyn StdError>> {
    Ok(HistoryEntry {
        job_id: Uuid::from_u128(id),
        printed_at: "2026-10-16T10:00:00Z".parse()?,
        receipt_number: Some(id.to_string()),
        total: Grosze::new(100)?,
        payments: vec![
            (PaymentForm::Card, Grosze::new(40)?),
            (PaymentForm::Credit, Grosze::new(60)?),
        ],
        items: vec!["Zażółć gęślą jaźń".to_owned()],
    })
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn torn_history_retains_complete_receipts_once_and_accepts_the_next_receipt() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("Historia – Łódź"))?;
    fs::write(
        dir.file("history.jsonl"),
        include_bytes!("fixtures/history-torn.jsonl"),
    )?;
    let snapshot = append(&dir, &entry(3)?)?;
    assert_eq!(
        snapshot.as_slice(),
        [entry(3)?, entry(2)?, entry(1)?].as_slice(),
        "torn rows are skipped, complete duplicate upgrades keep the original position"
    );
    drop(dir);
    let reopened = StateDir::open(temporary.path().join("Historia – Łódź"))?;
    assert_eq!(
        load(&reopened)?,
        [entry(3)?, entry(2)?, entry(1)?],
        "repaired compact history survives restart"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn staff_can_upgrade_a_missing_number_without_replacing_print_evidence() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let mut unnumbered = entry(1)?;
    unnumbered.receipt_number = None;
    let _snapshot = append(&dir, &unnumbered)?;
    let mut supplied = entry(1)?;
    supplied.items = vec!["server text must not overwrite printed evidence".to_owned()];
    let _snapshot = append(&dir, &supplied)?;
    let mut expected = unnumbered;
    expected.receipt_number = Some("1".to_owned());
    assert_eq!(
        load(&dir)?,
        [expected.clone()],
        "staff resolution upgrades only the missing receipt number"
    );
    drop(dir);
    let reopened = StateDir::open(temporary.path().join("state"))?;
    assert_eq!(
        load(&reopened)?,
        [expected],
        "upgraded number must be durable"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn replay_cannot_duplicate_a_receipt_or_change_an_existing_number() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let _snapshot = append(&dir, &entry(1)?)?;
    let _snapshot = append(&dir, &entry(2)?)?;
    let mut replay = entry(1)?;
    replay.receipt_number = Some("999".to_owned());
    let _snapshot = append(&dir, &replay)?;
    assert_eq!(
        load(&dir)?,
        [entry(2)?, entry(1)?],
        "recovery never duplicates or rewrites numbered receipt evidence"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn compaction_bounds_the_physical_file_and_retains_exactly_the_newest_receipts() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let mut bytes = Vec::new();
    for id in 1..=1000 {
        serde_json::to_writer(&mut bytes, &entry(id)?)?;
        bytes.push(b'\n');
    }
    fs::write(dir.file("history.jsonl"), bytes)?;
    let snapshot = append(&dir, &entry(1001)?)?;
    let expected = (502..=1001)
        .rev()
        .map(entry)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        snapshot.as_slice(),
        expected.as_slice(),
        "only the last five hundred distinct receipts are displayed"
    );
    let bytes = fs::read(dir.file("history.jsonl"))?;
    let stored = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice::<HistoryEntry>)
        .collect::<Result<Vec<_>, _>>()?;
    let chronological = (502..=1001).map(entry).collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        stored, chronological,
        "compaction removes old physical rows, not just old display rows"
    );
    let _snapshot = append(&dir, &entry(1002)?)?;
    assert_eq!(
        load(&dir)?,
        (503..=1002)
            .rev()
            .map(entry)
            .collect::<Result<Vec<_>, _>>()?,
        "subsequent appends remain bounded"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn a_previous_worker_snapshot_stays_unchanged_after_the_next_append() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let previous = append(&dir, &entry(1)?)?;
    let current = append(&dir, &entry(2)?)?;
    assert_eq!(
        previous,
        [entry(1)?],
        "a published snapshot must remain unchanged while the worker changes history"
    );
    assert_eq!(
        current,
        [entry(2)?, entry(1)?],
        "the next snapshot contains the new receipt, newest first"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn unreadable_history_returns_an_append_error_not_a_publishable_snapshot() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    fs::create_dir(dir.file("history.jsonl"))?;
    assert!(
        matches!(
            append(&dir, &entry(2)?),
            Err(Error::Store {
                action: "Nie można zapisać historii",
                ..
            })
        ),
        "an unsuccessful append must not return a history snapshot"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn unreadable_history_is_reported_instead_of_silently_discarded() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    fs::create_dir(dir.file("history.jsonl"))?;
    assert!(
        matches!(
            load(&dir),
            Err(Error::Store {
                action: "Nie można odczytać historii",
                ..
            })
        ),
        "a storage error is distinct from an empty cache"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn a_history_write_repairs_an_existing_world_readable_file() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    fs::write(
        dir.file("history.jsonl"),
        include_bytes!("fixtures/history-torn.jsonl"),
    )?;
    fs::set_permissions(dir.file("history.jsonl"), fs::Permissions::from_mode(0o644))?;
    let _snapshot = append(&dir, &entry(3)?)?;
    assert_eq!(
        fs::metadata(dir.file("history.jsonl"))?
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "customer history is owner-only after replacement"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn newly_created_history_is_owner_only() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let _snapshot = append(&dir, &entry(1)?)?;
    assert_eq!(
        fs::metadata(dir.file("history.jsonl"))?
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "new history is private regardless of umask"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn loading_history_tightens_its_permissions_without_waiting_for_a_print() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    fs::write(
        dir.file("history.jsonl"),
        include_bytes!("fixtures/history-torn.jsonl"),
    )?;
    fs::set_permissions(dir.file("history.jsonl"), fs::Permissions::from_mode(0o644))?;
    assert_eq!(
        load(&dir)?,
        [entry(2)?, entry(1)?],
        "complete current-format receipts stay readable"
    );
    assert_eq!(
        fs::metadata(dir.file("history.jsonl"))?
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "privacy repair must not depend on issuing another receipt"
    );
    Ok(())
}
