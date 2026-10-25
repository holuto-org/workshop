use std::{error::Error as StdError, fs};

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
        (" 12\u{a0}34 ", "12 34"),
        ("１２３／２０２６", "123/2026"),
        (" A\u{2003}\u{2003}B ", "A B"),
        ("12\u{200b}34", "12\u{200b}34"),
    ] {
        let device = Device::Completed {
            receipt_number: Some(raw.to_owned()),
        };
        journal.save(&Operation::Receipt(ReceiptOperation {
            result: Some(JobResult::Done {
                receipt_number: raw.to_owned(),
            }),
            ..receipt(device.clone())?
        }))?;
        let Some(Operation::Receipt(restored)) = journal.load()? else {
            return Err("missing receipt journal".into());
        };
        let form = restored.result_form().ok_or("missing result")?;
        assert_eq!(form.state, Outcome::Done);
        assert_eq!(form.receipt_number.as_deref(), Some(canonical));
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
fn new_completion_results_normalize_receipt_numbers() {
    for (raw, canonical) in [
        (" １２\u{a0}34 ".to_owned(), "12 34".to_owned()),
        ("1".repeat(64), "1".repeat(64)),
    ] {
        assert_eq!(
            JobResult::done(Some(raw), "Brak numeru."),
            JobResult::Done {
                receipt_number: canonical
            }
        );
    }
}

#[test]
fn new_completion_results_require_staff_for_unusable_receipt_numbers() {
    assert_eq!(
        JobResult::done(None, "Brak numeru."),
        JobResult::Unknown {
            reason: "Brak numeru.".to_owned()
        }
    );
    for raw in [
        String::new(),
        " ".to_owned(),
        "1".repeat(65),
        "\u{fb03}".repeat(22),
        "12\u{200b}34".to_owned(),
        "12\u{0}34".to_owned(),
        "12\n34".to_owned(),
        "12\u{2028}34".to_owned(),
    ] {
        assert_eq!(
            JobResult::done(Some(raw), "Brak numeru."),
            JobResult::Unknown {
                reason: "Nieprawidłowy numer wydrukowanego paragonu. Uzupełnij numer w Holuto."
                    .to_owned()
            }
        );
    }
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

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove durable one-time identity migration."
)]
fn legacy_identity_keys_migrate_without_losing_journal_evidence() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    for original in [
        include_str!("fixtures/receipt-dispatching.json"),
        include_str!("fixtures/report-dispatching.json"),
    ] {
        let legacy = original.replace("device_unique_number", "device_serial");
        fs::write(dir.file("operation.json"), &legacy)?;
        let journal = Journal::new(&dir);
        let operation = journal
            .load()?
            .expect("A valid legacy record must remain recoverable");
        let saved = fs::read_to_string(dir.file("operation.json"))?;
        assert!(!saved.contains("\"device_serial\""));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&saved)?,
            serde_json::to_value(operation)?
        );
        for corrupt in [
            legacy.replace("POS0000123456", "OLD"),
            legacy.replace(
                "\"device_serial\":",
                "\"device_unique_number\":\"POS0000123456\",\"device_serial\":",
            ),
        ] {
            fs::write(dir.file("operation.json"), &corrupt)?;
            assert!(matches!(journal.load(), Err(crate::Error::Blocked(_))));
            assert_eq!(fs::read_to_string(dir.file("operation.json"))?, corrupt);
        }
    }
    Ok(())
}
