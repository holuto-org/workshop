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
use crate::{api::model::JobState, store::StateDir};

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
    reason = "Assertions prove invalid journals block and stay untouched."
)]
fn malformed_or_unsupported_journals_are_never_repaired() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let receipt = include_str!("fixtures/receipt-dispatching.json");
    let report = include_str!("fixtures/report-dispatching.json");
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
        report.replace("    \"acknowledged_at\": null\n", ""),
        report.replace("\"report\": {", "\"report\": {\"extra\": true,"),
        "{".to_owned(),
    ] {
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
        let mut journal = Journal::new(&dir);
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
