use std::{error::Error as StdError, fs, io::ErrorKind};

use fiscal::{
    device::Vendor,
    money::Grosze,
    receipt::{Payment, PaymentForm},
    report::Report,
};
use uuid::Uuid;

use super::{
    Device, JobResult, Journal, Operation, ReceiptOperation, ReportOperation, Settlement, Summary,
};
use crate::{
    api::model::{JobState, Outcome},
    store::StateDir,
};

type TestResult = Result<(), Box<dyn StdError>>;

fn receipt(device: Device) -> Result<ReceiptOperation, Box<dyn StdError>> {
    let summary = if device == Device::NotStarted {
        None
    } else {
        Some(Summary {
            total: Grosze::new(100)?,
            payments: vec![Payment {
                form: PaymentForm::Card,
                amount: Grosze::new(100)?,
            }],
            items: vec!["Zażółć gęślą jaźń".to_owned()],
        })
    };
    Ok(ReceiptOperation {
        job_id: Uuid::from_u128(1),
        attempt: Uuid::from_u128(2),
        issued_at: "2026-10-16T10:00:00Z".parse()?,
        vendor: Vendor::Posnet,
        device_unique_number: "POS0000123456".to_owned(),
        device,
        summary,
        result: None,
        settled: None,
    })
}

fn report(report: Report, device: Device) -> Result<ReportOperation, Box<dyn StdError>> {
    Ok(ReportOperation {
        report,
        vendor: Vendor::Posnet,
        device_unique_number: "POS0000123456".to_owned(),
        started_at: "2026-10-16T10:00:00Z".parse()?,
        device,
        acknowledged_at: None,
    })
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The current-format golden table enumerates every durable operation state."
)]
fn every_durable_operation_state_reads_and_writes_its_reviewed_golden_file() -> TestResult {
    let numbered = Device::Completed {
        receipt_number: Some("123/2026".to_owned()),
    };
    let dispatching = Device::Dispatching {
        stage: "trline".to_owned(),
    };
    let done = JobResult::Done {
        receipt_number: "123/2026".to_owned(),
    };
    let corpus = [
        (
            "receipt-not-started",
            include_bytes!("fixtures/receipt-not-started.json").as_slice(),
            Operation::Receipt(receipt(Device::NotStarted)?),
            true,
        ),
        (
            "receipt-dispatching",
            include_bytes!("fixtures/receipt-dispatching.json").as_slice(),
            Operation::Receipt(receipt(dispatching.clone())?),
            true,
        ),
        (
            "receipt-completed-numbered",
            include_bytes!("fixtures/receipt-completed-numbered.json").as_slice(),
            Operation::Receipt(receipt(numbered.clone())?),
            true,
        ),
        (
            "receipt-completed-unnumbered",
            include_bytes!("fixtures/receipt-completed-unnumbered.json").as_slice(),
            Operation::Receipt(receipt(Device::Completed {
                receipt_number: None,
            })?),
            true,
        ),
        (
            "receipt-result-done",
            include_bytes!("fixtures/receipt-result-done.json").as_slice(),
            Operation::Receipt(ReceiptOperation {
                result: Some(done.clone()),
                ..receipt(numbered.clone())?
            }),
            true,
        ),
        (
            "receipt-result-failed",
            include_bytes!("fixtures/receipt-result-failed.json").as_slice(),
            Operation::Receipt(ReceiptOperation {
                result: Some(JobResult::Failed {
                    reason: "Odmowa drukarki: pamięć pełna.".to_owned(),
                }),
                ..receipt(Device::NotStarted)?
            }),
            true,
        ),
        (
            "receipt-result-unknown",
            include_bytes!("fixtures/receipt-result-unknown.json").as_slice(),
            Operation::Receipt(ReceiptOperation {
                result: Some(JobResult::Unknown {
                    reason: "Sprawdź wydruk – odpowiedź utracona.".to_owned(),
                }),
                ..receipt(dispatching)?
            }),
            true,
        ),
        (
            "receipt-settled",
            include_bytes!("fixtures/receipt-settled.json").as_slice(),
            Operation::Receipt(ReceiptOperation {
                result: Some(done),
                settled: Some(Settlement {
                    at: "2026-10-16T10:01:00Z".parse()?,
                    state: JobState::Done,
                }),
                ..receipt(numbered)?
            }),
            false,
        ),
        (
            "report-not-started",
            include_bytes!("fixtures/report-not-started.json").as_slice(),
            Operation::Report(report(
                Report::Monthly {
                    year: 2026,
                    month: 9,
                },
                Device::NotStarted,
            )?),
            false,
        ),
        (
            "report-dispatching",
            include_bytes!("fixtures/report-dispatching.json").as_slice(),
            Operation::Report(report(
                Report::Periodic {
                    from: "2026-09-01".parse()?,
                    to: "2026-09-30".parse()?,
                    summary: false,
                },
                Device::Dispatching {
                    stage: "periodic".to_owned(),
                },
            )?),
            true,
        ),
        (
            "report-completed",
            include_bytes!("fixtures/report-completed.json").as_slice(),
            Operation::Report(report(
                Report::Daily,
                Device::Completed {
                    receipt_number: None,
                },
            )?),
            false,
        ),
        (
            "report-acknowledged",
            include_bytes!("fixtures/report-acknowledged.json").as_slice(),
            Operation::Report(ReportOperation {
                acknowledged_at: Some("2026-10-16T10:02:00Z".parse()?),
                ..report(
                    Report::Daily,
                    Device::Dispatching {
                        stage: "dailyrep".to_owned(),
                    },
                )?
            }),
            false,
        ),
    ];
    for (name, golden, expected, blocks) in corpus {
        let temporary = tempfile::tempdir()?;
        let path = temporary.path().join("Rejestr – Łódź");
        let dir = StateDir::open(path.clone())?;
        fs::write(dir.file("operation.json"), golden)?;
        let journal = Journal::new(&dir);
        let restored = journal.load()?;
        assert_eq!(
            restored,
            Some(expected.clone()),
            "stored recovery evidence must retain every field: {name}"
        );
        assert_eq!(
            restored.as_ref().is_some_and(Operation::blocks),
            blocks,
            "recovery must preserve the fixture's blocked/settled state: {name}"
        );
        fs::remove_file(dir.file("operation.json"))?;
        journal.save(&expected)?;
        assert_eq!(
            fs::read(dir.file("operation.json"))?,
            golden,
            "writing a recovered operation must preserve its reviewed stored format: {name}"
        );
        drop(dir);
        let reopened = StateDir::open(path)?;
        assert_eq!(
            Journal::new(&reopened).load()?,
            Some(expected),
            "persisted recovery evidence must survive an instance restart: {name}"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn persisted_result_notes_keep_polish_text_but_never_backend_forbidden_controls() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let journal = Journal::new(&dir);
    for (input, reason) in [
        ("a\u{0}b\r\nc\td".to_owned(), "ab\nc\td".to_owned()),
        (" \u{7} ".to_owned(), "Brak opisu błędu.".to_owned()),
        ("ż".repeat(2001), "ż".repeat(2000)),
        (
            " a\u{ad}\u{34f}\u{61c}\u{200b}\u{200e}\u{200f}\u{202a}\u{202e}\
             \u{2060}\u{feff}\u{fdd0}\u{ffff}\u{10ffff}b "
                .to_owned(),
            "ab".to_owned(),
        ),
        (
            "Zażo\u{301}łć\u{2028}gęślą\u{2029}jaźń".to_owned(),
            "Zażółć\ngęślą\njaźń".to_owned(),
        ),
        (
            "a\u{200c}\u{200d}\u{fe0f}\u{e0100}\u{e0020}\u{e007f}👍🏻b".to_owned(),
            "a\u{200c}\u{200d}\u{fe0f}\u{e0100}\u{e0020}\u{e007f}👍🏻b".to_owned(),
        ),
    ] {
        for (result, expected) in [
            (
                JobResult::failed(&input),
                JobResult::Failed {
                    reason: reason.clone(),
                },
            ),
            (
                JobResult::unknown(&input),
                JobResult::Unknown {
                    reason: reason.clone(),
                },
            ),
        ] {
            journal.save(&Operation::Receipt(ReceiptOperation {
                result: Some(result),
                ..receipt(Device::NotStarted)?
            }))?;
            assert_eq!(
                journal.load()?,
                Some(Operation::Receipt(ReceiptOperation {
                    result: Some(expected),
                    ..receipt(Device::NotStarted)?
                })),
                "persisted API notes must satisfy the backend text contract"
            );
        }
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove canonical API evidence and unchanged raw printer evidence."
)]
fn journaled_done_sends_a_canonical_receipt_number() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let journal = Journal::new(&dir);
    for (raw, canonical) in [
        (" 12\u{a0}34 ".to_owned(), "12 34".to_owned()),
        ("１２３／２０２６".to_owned(), "123/2026".to_owned()),
        (" A\u{2003}\u{2003}B ".to_owned(), "A B".to_owned()),
        ("1".repeat(64), "1".repeat(64)),
    ] {
        let device = Device::Completed {
            receipt_number: Some(raw.clone()),
        };
        journal.save(&Operation::Receipt(ReceiptOperation {
            result: Some(JobResult::done(Some(raw))),
            ..receipt(device.clone())?
        }))?;
        let Some(Operation::Receipt(restored)) = journal.load()? else {
            return Err("missing receipt journal".into());
        };
        let form = restored.result_form().ok_or("missing result")?;
        assert_eq!(form.state, Outcome::Done);
        assert_eq!(form.receipt_number, Some(canonical));
        assert_eq!(form.issued_at, Some(restored.issued_at));
        assert_eq!(form.result, None);
        assert_eq!(
            restored.device, device,
            "raw fiscal evidence must remain intact"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Persisted result assertions use Result for setup."
)]
fn unusable_completion_numbers_preserve_raw_evidence_and_request_staff_resolution() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    for raw in [
        None,
        Some(String::new()),
        Some(" ".to_owned()),
        Some("1".repeat(65)),
        Some("\u{fb03}".repeat(22)),
        Some("12\u{200b}34".to_owned()),
        Some("12\u{0}34".to_owned()),
        Some("12\n34".to_owned()),
        Some("12\u{2028}34".to_owned()),
    ] {
        let expected = if raw.is_none() {
            "Wydruk zakończony, ale nie odczytano numeru paragonu. Sprawdź wydruk i rozstrzygnij \
             zadanie w Holuto."
        } else {
            "Nieprawidłowy numer wydrukowanego paragonu. Uzupełnij numer w Holuto."
        };
        let device = Device::Completed {
            receipt_number: raw.clone(),
        };
        let journal = Journal::new(&dir);
        journal.save(&Operation::Receipt(ReceiptOperation {
            result: Some(JobResult::done(raw)),
            ..receipt(device.clone())?
        }))?;
        let Some(Operation::Receipt(restored)) = journal.load()? else {
            return Err("missing receipt evidence".into());
        };
        assert_eq!(restored.device, device);
        assert_eq!(format!("{:?}", restored.device), "Completed([REDACTED])");
        assert_eq!(
            format!("{:?}", restored.result.as_ref().ok_or("missing result")?),
            "Unknown([REDACTED])"
        );
        let form = restored.result_form().ok_or("missing result form")?;
        assert_eq!(form.state, Outcome::Unknown);
        assert_eq!(form.receipt_number, None);
        assert_eq!(form.issued_at, None);
        assert_eq!(form.result.as_deref(), Some(expected));
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove invalid journals block and stay untouched."
)]
fn malformed_or_unsupported_journals_are_never_repaired() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let receipt = include_str!("fixtures/receipt-dispatching.json");
    let mut cases = Vec::new();
    for (fixture, pointer, key, replacement) in [
        (receipt, "/receipt", "summary", None),
        (receipt, "/receipt", "result", None),
        (receipt, "/receipt", "settled", None),
        (
            include_str!("fixtures/receipt-completed-unnumbered.json"),
            "/receipt/device/completed",
            "receipt_number",
            None,
        ),
        (
            include_str!("fixtures/report-dispatching.json"),
            "/report",
            "acknowledged_at",
            None,
        ),
        (
            include_str!("fixtures/report-dispatching.json"),
            "/report",
            "extra",
            Some(serde_json::Value::Bool(true)),
        ),
        (
            include_str!("fixtures/report-dispatching.json"),
            "/report/report",
            "extra",
            Some(serde_json::Value::Bool(true)),
        ),
    ] {
        let mut json: serde_json::Value = serde_json::from_str(fixture)?;
        let object = json
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("fixture object missing")?;
        if let Some(value) = replacement {
            assert_eq!(object.insert(key.to_owned(), value), None);
        } else {
            assert!(
                object.remove(key).is_some(),
                "mutation must remove {pointer}/{key}"
            );
        }
        cases.push(serde_json::to_string(&json)?);
    }
    for text in [
        receipt.replace("\"vendor\": \"posnet\"", "\"vendor\": null"),
        receipt.replace("\"vendor\": \"posnet\"", "\"vendor\": \"unknown\""),
        receipt.replace("\"vendor\": \"posnet\"", "\"vendor\": 1"),
        receipt.replace("    \"vendor\": \"posnet\",\n", ""),
        receipt.replace("\"receipt\": {", "\"receipt\": {\"extra\": true,"),
        receipt.replace("\"summary\": {", "\"summary\": {\"extra\": true,"),
        receipt.replace(
            "\"form\": \"card\",",
            "\"form\": \"card\", \"extra\": true,",
        ),
        receipt.replace("\"dispatching\": {", "\"dispatching\": {\"extra\": true,"),
        receipt.replace(
            "    \"settled\": null\n",
            "    \"settled\": null, \"settled\": null\n",
        ),
        receipt.replace("\"stage\": \"trline\"", "\"stage\": false"),
        "{".to_owned(),
    ]
    .into_iter()
    .chain(cases)
    {
        fs::write(dir.file("operation.json"), &text)?;
        assert!(
            matches!(Journal::new(&dir).load(), Err(crate::Error::Blocked(_))),
            "{text}"
        );
        assert_eq!(fs::read(dir.file("operation.json"))?, text.as_bytes());
    }
    Ok(())
}

/// The golden files hold one settlement state and payment form; the journal may hold the
/// others, whose names come from the wire and receipt types.
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn every_stored_settlement_state_and_payment_form_keeps_its_name() -> TestResult {
    for (state, name) in [
        (JobState::Failed, "failed"),
        (JobState::Expired, "expired"),
        (JobState::Voided, "voided"),
    ] {
        let stored = serde_json::json!({"at": "2026-10-16T10:01:00Z", "state": name});
        let settlement = Settlement {
            at: "2026-10-16T10:01:00Z".parse()?,
            state,
        };
        assert_eq!(serde_json::to_value(&settlement)?, stored);
        assert_eq!(serde_json::from_value::<Settlement>(stored)?, settlement);
    }
    for (form, name) in [
        (PaymentForm::Cash, "cash"),
        (PaymentForm::Transfer, "transfer"),
        (PaymentForm::Mobile, "mobile"),
        (PaymentForm::Credit, "credit"),
    ] {
        let stored = serde_json::json!({"form": name, "amount": 100});
        let payment = Payment {
            form,
            amount: Grosze::new(100)?,
        };
        assert_eq!(serde_json::to_value(&payment)?, stored);
        assert_eq!(serde_json::from_value::<Payment>(stored)?, payment);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Failed writes must preserve exact fiscal evidence."
)]
fn failed_journal_replacement_retains_previous_evidence_across_restart() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("state");
    let dir = StateDir::open(path.clone())?;
    let previous = Operation::Receipt(receipt(Device::Dispatching {
        stage: "trline".to_owned(),
    })?);
    let journal = Journal::new(&dir);
    journal.save(&previous)?;
    let bytes = fs::read(dir.file("operation.json"))?;
    let next = Operation::Receipt(receipt(Device::Completed {
        receipt_number: Some("123".to_owned()),
    })?);
    journal.fail_write_after(0);
    let error = journal
        .save(&next)
        .err()
        .ok_or("injected failure ignored")?;
    let crate::Error::Store {
        action,
        path: failed_path,
        source,
    } = error
    else {
        return Err("write failure must remain a storage error".into());
    };
    assert_eq!(action, "Nie można zapisać rejestru operacji");
    assert_eq!(failed_path, dir.file("operation.json"));
    assert_eq!(source.kind(), ErrorKind::PermissionDenied);
    assert_eq!(fs::read(dir.file("operation.json"))?, bytes);
    assert_eq!(journal.load()?, Some(previous.clone()));
    drop(dir);
    let reopened = StateDir::open(path)?;
    let restored = Journal::new(&reopened).load()?.ok_or("lost journal")?;
    assert_eq!(restored, previous);
    assert!(restored.blocks());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unreadable evidence is never reset as recovery."
)]
fn unreadable_journal_blocks_without_removing_its_contents() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let path = dir.file("operation.json");
    fs::create_dir(&path)?;
    fs::write(path.join("evidence"), b"keep fiscal record")?;
    assert!(matches!(
        Journal::new(&dir).load(),
        Err(crate::Error::Store {
            action: "Nie można odczytać rejestru operacji. Drukowanie zablokowane",
            ..
        })
    ));
    assert_eq!(fs::read(path.join("evidence"))?, b"keep fiscal record");
    Ok(())
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unix journal privacy assertions use Result."
)]
fn journal_creation_and_replacement_are_private_but_reads_do_not_repair_modes() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let journal = Journal::new(&dir);
    let operation = Operation::Receipt(receipt(Device::NotStarted)?);
    journal.save(&operation)?;
    let path = dir.file("operation.json");
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
    assert_eq!(journal.load()?, Some(operation.clone()));
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o644);
    journal.save(&operation)?;
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    Ok(())
}
