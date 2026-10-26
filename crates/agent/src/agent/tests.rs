use std::{collections::BTreeMap, error::Error as StdError, fs, io::ErrorKind, str};

use fiscal::{device::Vendor, error::Error as PrinterError, money::Grosze, report::Report};
use posnet::{
    capabilities::capabilities,
    simulator::{Fault as DeviceFault, SimReport, Simulator, UNIQUE_NUMBER},
};
use serde_json::json;

use super::{Agent, Blocked};
use crate::{
    Error,
    api::{Api, ApiError, model::JobState},
    documents::{BuyerNip, Document},
    history::{self, HistoryEntry},
    journal::{
        Device, JobResult, Journal, Operation, ReceiptOperation, ReportOperation, Settlement,
        Summary,
    },
    store::StateDir,
    testing::{ALLOCATION_ID, ATTEMPT, Fault, Holuto, JOB_ID, job_json, state_dir, with},
};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn StdError>>;
fn waiting(_message: Option<&str>) {}
fn agent<'a>(sim: &Simulator, dir: &'a StateDir, server: &Holuto) -> Result<Agent<'a>, Error> {
    let api = Api::new(server.url(), server.token(), UNIQUE_NUMBER)?;
    let mut agent = Agent::new(sim.driver(), api, dir, &waiting);
    let status = agent.printer.probe()?;
    let _notices = agent.describe(status)?;
    let _connected = agent.session()?;
    Ok(agent)
}
fn claimed(
    device: Device,
    result: Option<JobResult>,
) -> Result<ReceiptOperation, Box<dyn StdError>> {
    Ok(ReceiptOperation {
        job_id: JOB_ID,
        attempt: ATTEMPT,
        issued_at: "2025-01-02T12:00:00Z".parse()?,
        vendor: Vendor::Posnet,
        device_unique_number: UNIQUE_NUMBER.to_owned(),
        device,
        summary: None,
        result,
        settled: None,
    })
}
fn stored(dir: &StateDir) -> Result<ReceiptOperation, Box<dyn StdError>> {
    match Journal::new(dir).load()? {
        Some(Operation::Receipt(operation)) => Ok(operation),
        other => Err(format!("expected receipt journal, got {other:?}").into()),
    }
}
/// Posnet frames naming any of `commands`.
fn sent(sim: &Simulator, commands: &[&str]) -> Vec<Vec<u8>> {
    sim.frames()
        .into_iter()
        .filter(|frame| {
            commands.iter().any(|command| {
                frame
                    .windows(command.len())
                    .any(|part| part == command.as_bytes())
            })
        })
        .collect()
}
const RECEIPT_COMMANDS: [&str; 8] = [
    "trinit",
    "trnipset",
    "trline",
    "tradvance",
    "trpayment",
    "trend",
    "trftrln",
    "trftrend",
];
#[test]
fn invalid_printer_description_never_reaches_the_session_endpoint() {
    for field in ["model", "firmware"] {
        for invalid in [
            String::new(),
            "x".repeat(65),
            "wersja\n".to_owned(),
            "\u{85}".to_owned(),
        ] {
            let sim = Simulator::start();
            let server = Holuto::new();
            let (_temporary, dir) = state_dir();
            let api = Api::new(server.url(), server.token(), UNIQUE_NUMBER).unwrap();
            let mut agent = Agent::new(sim.driver(), api, &dir, &waiting);
            let mut status = agent.printer.probe().unwrap();
            if field == "model" {
                status.model = invalid;
            } else {
                status.firmware = invalid;
            }
            let _notices = agent.describe(status).unwrap();
            let result = agent.session();
            let requests = server.requests();
            assert!(matches!(
                result,
                Err(Error::Invalid(message))
                    if message == "Nieprawidłowy model lub firmware drukarki: \
                        wymagane od 1 do 64 znaków bez znaków sterujących."
            ));
            assert!(requests.is_empty(), "{field}: invalid telemetry was sent");
            assert_eq!(sim.receipts(), []);
            assert_eq!(sim.reports(), []);
        }
    }
}

#[test]
fn printer_description_code_limit_counts_unicode_characters_not_bytes() {
    let sim = Simulator::start();
    let server = Holuto::new();
    let (_temporary, dir) = state_dir();
    let api = Api::new(server.url(), server.token(), UNIQUE_NUMBER).unwrap();
    let mut agent = Agent::new(sim.driver(), api, &dir, &waiting);
    let mut status = agent.printer.probe().unwrap();
    status.model = "Ą".repeat(64);
    status.firmware = "1".to_owned();
    let _notices = agent.describe(status).unwrap();
    assert!(agent.session().unwrap());
    assert_eq!(server.requests().len(), 1);
    assert_eq!(server.requests()[0].body["model"], "Ą".repeat(64));
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn printer_time_and_original_receipt_values_reach_holuto_and_history() -> TestResult {
    let sim = Simulator::start();
    sim.set_clock("2025-01-02T12:34:00Z".parse()?);
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    let _delay = agent.step()?;
    assert_eq!(
        server.job(JOB_ID)["state"],
        "done",
        "successful print is accepted by Holuto"
    );
    let result = server
        .requests()
        .into_iter()
        .find(|request| request.path.ends_with("/result"))
        .ok_or("missing result")?;
    assert_eq!(result.body["issuedAt"], "2025-01-02T12:34:00Z");
    let receipts = sim.receipts();
    let receipt = receipts.first().ok_or("missing physical receipt")?;
    assert_eq!(receipt.total, 25000);
    assert_eq!(
        receipt
            .lines
            .first()
            .ok_or("missing first line")?
            .get("na")
            .map(String::as_str),
        Some("Wymiana oleju")
    );
    assert_eq!(
        receipt
            .payments
            .first()
            .ok_or("missing card payment")?
            .get("ty")
            .map(String::as_str),
        Some("2")
    );
    assert_eq!(
        history::load(&dir)?
            .first()
            .ok_or("missing history")?
            .receipt_number
            .as_deref(),
        Some("1")
    );
    assert!(
        stored(&dir)?.settled.is_some(),
        "accepted result settles journal"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn not_started_expired_attempt_waits_for_staff_with_explicit_no_dispatch_notice() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(job_json());
    server.expire(JOB_ID);
    let (_temporary, dir) = state_dir();
    Journal::new(&dir).save(&Operation::Receipt(claimed(Device::NotStarted, None)?))?;
    let mut agent = agent(&sim, &dir, &server)?;
    assert_eq!(agent.settle()?, Some(Blocked::Receipt));
    assert_eq!(
        agent.take_news().notices,
        [format!(
            "Zlecenie 10 (zadanie {JOB_ID}): rejestr drukarki potwierdza, że nie wysłano żadnego \
             polecenia fiskalnego. Rozstrzygnij zadanie jako niewydrukowane w Holuto."
        )]
    );
    server.resolve(JOB_ID, None)?;
    assert_eq!(agent.settle()?, None);
    assert!(
        stored(&dir)?.settled.is_some(),
        "staff resolution unblocks without a command"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn an_expired_taken_attempt_accepts_completed_evidence_as_done() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(job_json());
    server.expire(JOB_ID);
    let (_temporary, dir) = state_dir();
    Journal::new(&dir).save(&Operation::Receipt(claimed(
        Device::Completed {
            receipt_number: Some("41".to_owned()),
        },
        None,
    )?))?;
    let mut agent = agent(&sim, &dir, &server)?;
    assert_eq!(agent.settle()?, None);
    assert_eq!(server.job(JOB_ID)["state"], "done");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn unchanged_result_is_resent_after_rejection_before_commit_and_restart() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    server.fault(
        "POST",
        &format!("/agent/jobs/{JOB_ID}/result"),
        Fault::Problem {
            status: 503,
            code: "unavailable".to_owned(),
            detail: "temporary".to_owned(),
        },
    );
    let mut first = agent(&sim, &dir, &server)?;
    assert!(
        matches!(first.step(), Err(Error::Api(_))),
        "rejected result leaves durable outbox"
    );
    assert_eq!(server.job(JOB_ID)["state"], "taken");
    let journal = stored(&dir)?;
    let first_form = journal.result_form().ok_or("no outbox result")?;
    drop(first);
    let mut recovered = agent(&sim, &dir, &server)?;
    assert_eq!(recovered.settle()?, None);
    let result_requests: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|request| request.path.ends_with("/result"))
        .collect();
    assert_eq!(
        result_requests.len(),
        2,
        "recovery resends the rejected result"
    );
    assert_eq!(
        result_requests.first().ok_or("first result missing")?.body,
        serde_json::to_value(&first_form)?
    );
    assert_eq!(
        result_requests.last().ok_or("replay missing")?.body,
        serde_json::to_value(&first_form)?
    );
    assert_eq!(server.job(JOB_ID)["state"], "done");
    assert_eq!(
        sim.receipts().len(),
        1,
        "outbox replay is never a second print"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn committed_result_with_lost_acknowledgement_settles_after_restart_without_reprinting()
-> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    server.fault(
        "POST",
        &format!("/agent/jobs/{JOB_ID}/result"),
        Fault::DropAfterCommit,
    );
    let mut first = agent(&sim, &dir, &server)?;
    assert!(
        matches!(first.step(), Err(Error::Api(_))),
        "missing acknowledgement is observable"
    );
    assert_eq!(server.job(JOB_ID)["state"], "done");
    let evidence = stored(&dir)?;
    drop(first);
    let mut recovered = agent(&sim, &dir, &server)?;
    assert_eq!(recovered.settle()?, None);
    assert_eq!(stored(&dir)?.result, evidence.result);
    assert_eq!(
        sim.receipts().len(),
        1,
        "lost server acknowledgement never reprints"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn terminal_failed_or_expired_print_needs_same_device_local_acknowledgement() -> TestResult {
    for state in ["failed", "expired"] {
        let sim = Simulator::start();
        let server = Holuto::new();
        server.insert(with(job_json(), &json!({"state":state})));
        let (_temporary, dir) = state_dir();
        Journal::new(&dir).save(&Operation::Receipt(claimed(
            Device::Completed {
                receipt_number: Some("41".to_owned()),
            },
            None,
        )?))?;
        let mut agent = agent(&sim, &dir, &server)?;
        assert_eq!(agent.settle()?, Some(Blocked::OrphanedReceipt));
        let state_label = if state == "failed" {
            "Failed"
        } else {
            "Expired"
        };
        assert_eq!(
            agent.take_news().notices,
            [format!(
                "Paragon 41 został wydrukowany dla zlecenia 10 (zadanie {JOB_ID}), ale Holuto \
                 oznaczyło zadanie jako {state_label}. Sprawdź wydruk i użyj „Wyjaśnij paragon”."
            )]
        );
        sim.set_paper(false);
        assert!(
            matches!(agent.acknowledge_orphaned_receipt(), Err(Error::Printer(_))),
            "an unready printer cannot confirm the evidence"
        );
        assert_eq!(
            stored(&dir)?.settled,
            None,
            "a refused acknowledgement keeps the receipt blocking"
        );
        sim.set_paper(true);
        let requests_before = server.requests().len();
        agent.acknowledge_orphaned_receipt()?;
        assert!(
            stored(&dir)?.settled.is_some(),
            "explicit local acknowledgement unblocks"
        );
        assert!(
            server
                .requests()
                .iter()
                .skip(requests_before)
                .all(|request| request.method == "GET"),
            "acknowledgement never reports a forged server result"
        );
        assert!(sim.receipts().is_empty(), "acknowledgement never prints");
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn orphaned_print_refuses_reports_with_the_local_acknowledgement_not_holuto() -> TestResult {
    // Holuto staff can resolve only unknown jobs; this one is already failed.
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(with(job_json(), &json!({"state": "failed"})));
    let (_temporary, dir) = state_dir();
    Journal::new(&dir).save(&Operation::Receipt(claimed(
        Device::Completed {
            receipt_number: Some("41".to_owned()),
        },
        None,
    )?))?;
    let mut agent = agent(&sim, &dir, &server)?;
    assert!(matches!(
        agent.run_report(Report::Daily),
        Err(Error::Blocked(message))
            if message == "Najpierw wyjaśnij wydrukowany paragon przyciskiem „Wyjaśnij paragon”."
    ));
    assert_eq!(sim.reports(), []);
    assert_eq!(stored(&dir)?.settled, None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn different_attempt_cannot_settle_or_reprint_a_journal() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(with(
        job_json(),
        &json!({"attempt":"00000000-0000-0000-0000-000000000099"}),
    ));
    let (_temporary, dir) = state_dir();
    let operation = claimed(Device::NotStarted, None)?;
    Journal::new(&dir).save(&Operation::Receipt(operation.clone()))?;
    let mut agent = agent(&sim, &dir, &server)?;
    assert!(matches!(agent.settle(), Err(Error::Blocked(message))
            if message == "Nie można uzgodnić niepotwierdzonego paragonu z Holuto: \
                           Holuto zna inną próbę wykonania tego zadania. Drukowanie zablokowane."));
    assert_eq!(stored(&dir)?, operation);
    assert!(sim.receipts().is_empty(), "attempt mismatch never prints");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn claimed_job_needing_unqualified_capability_fails_without_fiscal_commands() -> TestResult {
    for (advances, zero, precision, changes, reason) in [
        (
            false,
            true,
            8,
            json!({
                "advances": [{"vatLetter": "A", "vatRate": "23", "amount": "100.00"}],
                "payments": [{"form": "card", "amount": "150.00"}]
            }),
            "Drukarka nie obsługuje rozliczenia zaliczek.",
        ),
        (
            true,
            false,
            8,
            json!({
                "advances": [
                    {"vatLetter": "A", "vatRate": "23", "amount": "200.00"},
                    {"vatLetter": "B", "vatRate": "8", "amount": "50.00"}
                ], "payments": []
            }),
            "Drukarka nie obsługuje paragonu opłaconego w całości zaliczkami.",
        ),
        (
            true,
            true,
            3,
            json!({
                "lines": [{"name": "Usługa", "quantity": "1.0001", "unitPrice": "250.00",
                    "total": "250.03", "vatRate": "23", "vatLetter": "A"}],
                "total": "250.03", "payments": [{"form": "card", "amount": "250.03"}]
            }),
            "Ilość może mieć najwyżej 3 miejsc po przecinku.",
        ),
    ] {
        let sim = Simulator::start();
        let mut capabilities = capabilities();
        capabilities.advance_deductions = advances;
        capabilities.zero_total_receipts = zero;
        capabilities.max_quantity_decimals = precision;
        sim.set_capabilities(capabilities);
        let server = Holuto::new();
        let _id = server.enqueue(with(job_json(), &changes));
        let (_temporary, dir) = state_dir();
        let mut agent = agent(&sim, &dir, &server)?;
        let _delay = agent.step()?;
        assert_eq!(server.job(JOB_ID)["state"], "failed");
        assert_eq!(server.job(JOB_ID)["result"], reason);
        let operation = stored(&dir)?;
        assert_eq!(operation.device, Device::NotStarted);
        assert_eq!(operation.result, Some(JobResult::failed(reason)));
        assert_eq!(
            operation.settled.as_ref().map(|value| value.state),
            Some(JobState::Failed)
        );
        assert_eq!(sim.receipts(), []);
        assert_eq!(sent(&sim, &RECEIPT_COMMANDS), Vec::<Vec<u8>>::new());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn oversized_claim_fails_without_fiscal_commands_and_frees_the_queue() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let line = json!({
        "name": "Pozycja", "quantity": "1", "unitPrice": "0.01",
        "total": "0.01", "vatRate": "23", "vatLetter": "A"
    });
    let changes = json!({"lines": vec![line.clone(); 501], "total": "5.01"});
    let oversized = server.enqueue(with(job_json(), &changes));
    let next = server.enqueue(with(
        job_json(),
        &json!({
            "id": "00000000-0000-0000-0000-000000000063",
            "lines": vec![line; 500],
            "total": "5.00",
            "payments": [{"form": "card", "amount": "5.00"}]
        }),
    ));
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    let _delay = agent.step()?;
    let failed = server.job(oversized);
    assert_eq!(
        failed["state"], "failed",
        "a certain failure, not a stalled claim"
    );
    assert_eq!(
        failed["result"],
        "Nieprawidłowe zadanie z Holuto: zadanie musi mieć od 1 do 500 pozycji."
    );
    assert_eq!(stored(&dir)?.device, Device::NotStarted);
    assert!(sim.receipts().is_empty(), "invalid job never prints");
    let _delay = agent.step()?;
    assert_eq!(
        server.job(next)["state"],
        "done",
        "the next job is claimed and printed"
    );
    assert_eq!(sim.receipts().len(), 1);
    let receipts = sim.receipts();
    let receipt = receipts.first().ok_or("missing maximum-size receipt")?;
    assert_eq!(receipt.total, 500);
    assert_eq!(
        receipt.lines,
        vec![
            BTreeMap::from([
                ("na".to_owned(), "Pozycja".to_owned()),
                ("il".to_owned(), "1".to_owned()),
                ("vt".to_owned(), "0".to_owned()),
                ("pr".to_owned(), "1".to_owned()),
                ("wa".to_owned(), "1".to_owned()),
            ]);
            500
        ]
    );
    assert_eq!(
        receipt.payments,
        [BTreeMap::from([
            ("ty".to_owned(), "2".to_owned()),
            ("wa".to_owned(), "500".to_owned()),
        ])]
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn malformed_claim_with_lost_failure_acknowledgement_settles_after_restart() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(with(job_json(), &json!({"buyerNip": "123"})));
    server.fault(
        "POST",
        &format!("/agent/jobs/{JOB_ID}/result"),
        Fault::DropAfterCommit,
    );
    let (_temporary, dir) = state_dir();
    let mut first = agent(&sim, &dir, &server)?;
    assert!(
        matches!(first.step(), Err(Error::Api(_))),
        "lost acknowledgement"
    );
    assert_eq!(server.job(JOB_ID)["state"], "failed");
    drop(first);
    let mut recovered = agent(&sim, &dir, &server)?;
    assert_eq!(
        recovered.settle()?,
        None,
        "job shape never blocks reconciliation"
    );
    assert!(stored(&dir)?.settled.is_some());
    assert!(sim.receipts().is_empty(), "a refused claim never prints");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn invalid_money_vat_or_text_fails_before_fiscal_commands() -> TestResult {
    for (changes, reason) in [
        (
            json!({"total":"249.99"}),
            "Suma pozycji nie odpowiada wartości zadania.",
        ),
        (
            json!({"lines": [{
                "name": "Pozycja", "quantity": "1", "unitPrice": "250.001",
                "total": "250.00", "vatRate": "23", "vatLetter": "A"
            }]}),
            "Cena: Kwota musi być zapisana cyframi, z najwyżej 2 miejscami po przecinku. \
             (\"250.001\")",
        ),
        (
            json!({"lines": [{
                "name": "Pozycja", "quantity": "1", "unitPrice": "250.00",
                "total": "250.00", "vatRate": "8", "vatLetter": "A"
            }]}),
            "Stawka VAT 8% zadania nie odpowiada slotowi 0 drukarki. Odśwież stawki w Holuto.",
        ),
        (
            json!({"lines": [
                {"name": "Wymiana oleju", "quantity": "2", "unitPrice": "100.00",
                 "total": "199.99", "vatRate": "23", "vatLetter": "A"},
                {"name": "Klocki hamulcowe", "quantity": "1", "unitPrice": "50.00",
                 "total": "50.01", "vatRate": "8", "vatLetter": "B"}
            ]}),
            "Wartość pozycji nie zgadza się z zaokrąglonym iloczynem ilości i ceny.",
        ),
        (
            json!({
                "advances": [{"vatLetter": "A", "vatRate": "8", "amount": "100.00"}],
                "payments": [{"form": "card", "amount": "150.00"}]
            }),
            "Stawka VAT 8% zadania nie odpowiada slotowi 0 drukarki. Odśwież stawki w Holuto.",
        ),
        (
            json!({"companyName":"Firma 🛠"}),
            "Nazwa firmy musi mieć od 1 do 40 znaków obsługiwanych przez drukarkę. Zmień ją \
             w profilu firmy w Holuto.",
        ),
    ] {
        let sim = Simulator::start();
        let server = Holuto::new();
        let _id = server.enqueue(with(job_json(), &changes));
        let (_temporary, dir) = state_dir();
        let mut agent = agent(&sim, &dir, &server)?;
        let _delay = agent.step()?;
        assert_eq!(
            server.job(JOB_ID)["state"],
            "failed",
            "invalid fiscal input gets a certain failure"
        );
        assert_eq!(server.job(JOB_ID)["result"], reason);
        assert_eq!(stored(&dir)?.result, Some(JobResult::failed(reason)));
        assert_eq!(sent(&sim, &RECEIPT_COMMANDS), Vec::<Vec<u8>>::new());
        assert!(sim.receipts().is_empty(), "invalid input never prints");
        assert_eq!(
            stored(&dir)?.device,
            Device::NotStarted,
            "validation precedes every fiscal command"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn corrupt_journal_blocks_receipts_and_reports_without_touching_printer() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    fs::write(dir.file("operation.json"), b"broken")?;
    let mut agent = agent(&sim, &dir, &server)?;
    let expected = "Uszkodzony rejestr operacji lub nieobsługiwany format. \
                    Drukowanie zablokowane; nie usuwaj pliku.";
    assert!(
        matches!(agent.step(), Err(Error::Blocked(message)) if message == expected),
        "corrupt journal blocks receipt"
    );
    let refused = agent.run_report(Report::Daily);
    assert!(
        matches!(refused, Err(Error::Blocked(message)) if message == expected),
        "corrupt journal blocks reports"
    );
    assert!(
        sim.receipts().is_empty() && sim.reports().is_empty(),
        "corrupt intent cannot reach device"
    );
    assert_eq!(fs::read(dir.file("operation.json"))?, b"broken");
    assert_eq!(server.job(JOB_ID)["state"], "queued");
    assert!(
        server
            .requests()
            .iter()
            .all(|request| request.path != "/agent/jobs/take")
    );
    assert_eq!(sent(&sim, &RECEIPT_COMMANDS), Vec::<Vec<u8>>::new());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn history_error_does_not_block_server_accepted_result() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    fs::create_dir(dir.file("history.jsonl"))?;
    let mut agent = agent(&sim, &dir, &server)?;
    let _delay = agent.step()?;
    let saved = stored(&dir)?;
    assert_eq!(
        saved.settled.map(|settled| settled.state),
        Some(JobState::Done)
    );
    assert_eq!(
        saved.device,
        Device::Completed {
            receipt_number: Some("1".to_owned())
        }
    );
    assert_eq!(server.job(JOB_ID)["state"], "done");
    assert_eq!(server.job(JOB_ID)["receiptNumber"], "1");
    assert_eq!(sim.receipts().len(), 1);
    let notices = agent.take_news().notices;
    assert_eq!(notices.len(), 1);
    let notice = notices.first().ok_or("missing history failure")?;
    let prefix = format!(
        "Nie można zapisać historii ({}): ",
        dir.file("history.jsonl").display()
    );
    assert!(
        notice
            .strip_prefix(&prefix)
            .is_some_and(|source| !source.is_empty()),
        "history failure identifies the cache and includes the OS error: {notice}"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn future_printer_clock_keeps_completed_evidence_and_surfaces_server_rejection() -> TestResult {
    let sim = Simulator::start();
    sim.set_clock("2099-01-02T12:00:00Z".parse()?);
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    assert!(
        matches!(
            agent.step(),
            Err(Error::Api(ApiError::Status { status: 400, .. }))
        ),
        "future printer time must not be silently clamped"
    );
    assert!(
        matches!(stored(&dir)?.device, Device::Completed { .. }),
        "physical result remains durable for staff"
    );
    assert!(
        stored(&dir)?.settled.is_none(),
        "refused evidence must remain blocked"
    );
    assert_eq!(
        sim.receipts().len(),
        1,
        "server rejection never causes a reprint"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn named_report_refusal_is_certain_and_does_not_block_following_work() -> TestResult {
    let sim = Simulator::start();
    sim.fault("dailyrep", DeviceFault::Error);
    let server = Holuto::new();
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    assert!(
        matches!(
            agent.run_report(Report::Daily),
            Err(Error::Printer(PrinterError::Device { code: 2000, .. }))
        ),
        "operator gets the device's certain report refusal"
    );
    assert_eq!(
        agent.blocked()?,
        None,
        "a refused report cannot be an unresolved print"
    );
    assert!(
        sim.reports().is_empty(),
        "refused report had no fiscal effect"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn unready_printer_refreshes_status_without_reopening_session_or_claiming_a_job() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    let requests_before = server.requests().len();
    sim.set_paper(false);
    let _delay = agent.step()?;
    assert_eq!(server.job(JOB_ID)["state"], "queued");
    let requests = server.requests();
    assert!(
        agent.status().is_some_and(|status| !status.ready()),
        "step refreshes the printer's paper-out status"
    );
    let requests = &requests[requests_before..];
    let heartbeat = requests
        .iter()
        .find(|request| request.path == "/agent/heartbeat")
        .ok_or("missing heartbeat")?;
    assert!(heartbeat.body.is_null());
    assert!(
        requests
            .iter()
            .all(|request| request.path != "/agent/session"),
        "paper-out leaves telemetry unchanged, so no new session is needed"
    );
    assert!(
        requests
            .iter()
            .all(|request| request.path != "/agent/jobs/take"),
        "readiness is checked before taking"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_claim_with_unwritable_journal_never_sends_a_fiscal_command() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    assert_eq!(
        agent.journal.load()?,
        None,
        "the journal read succeeds before dispatch"
    );
    agent.journal.fail_write_after(0);
    assert!(
        matches!(
            agent.step(),
            Err(Error::Store { source, .. }) if source.kind() == ErrorKind::PermissionDenied
        ),
        "journal failure prevents dispatch"
    );
    assert_eq!(
        server.job(JOB_ID)["state"],
        "taken",
        "the claim committed before its intent write failed"
    );
    assert_eq!(agent.journal.load()?, None, "no claim was persisted");
    assert_eq!(
        sent(&sim, &RECEIPT_COMMANDS),
        Vec::<Vec<u8>>::new(),
        "a failed claim WRITE prevents every fiscal command"
    );
    assert!(
        server
            .requests()
            .iter()
            .all(|request| !request.path.ends_with("/result")),
        "an unjournaled result cannot be reported"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_failed_dispatch_write_stops_the_receipt_before_its_first_fiscal_command() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    // The claim is the first write; the first command's intent is the second.
    agent.journal.fail_write_after(1);
    let _delay = agent.step()?;
    assert_eq!(
        sent(&sim, &RECEIPT_COMMANDS),
        Vec::<Vec<u8>>::new(),
        "an unjournaled command never reaches the printer"
    );
    assert_eq!(
        server.job(JOB_ID)["state"],
        "failed",
        "nothing was printed, so Holuto may requeue"
    );
    let operation = stored(&dir)?;
    assert_eq!(operation.device, Device::NotStarted);
    assert_eq!(
        operation.settled.map(|settlement| settlement.state),
        Some(JobState::Failed)
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_failed_dispatch_write_stops_the_report_before_it_prints() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    // A report's first write is its first command's intent.
    agent.journal.fail_write_after(0);
    assert!(matches!(
        agent.run_report(Report::Daily),
        Err(Error::Printer(PrinterError::Journal(source)))
            if source.kind() == ErrorKind::PermissionDenied
    ));
    assert_eq!(
        sent(&sim, &["dailyrep"]),
        Vec::<Vec<u8>>::new(),
        "an unjournaled report never reaches the printer"
    );
    assert_eq!(Journal::new(&dir).load()?, None);
    assert_eq!(
        agent.blocked()?,
        None,
        "a refused report leaves nothing to resolve"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_reoffered_job_is_never_printed_again() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let mut first = agent(&sim, &dir, &server)?;
    let _delay = first.step()?;
    let printed = stored(&dir)?;
    let frames = sent(&sim, &RECEIPT_COMMANDS);
    let history = history::load(&dir)?;
    let replay = server.job(JOB_ID);
    drop(first);
    server.fault(
        "POST",
        "/agent/jobs/take",
        Fault::Body {
            status: 200,
            body: serde_json::to_vec(&with(replay, &json!({"state": "taken"})))?,
        },
    );
    let mut recovered = agent(&sim, &dir, &server)?;
    assert!(matches!(
        recovered.step(),
        Err(Error::Blocked(message))
            if message == format!(
                "Zadanie {JOB_ID} było już wykonywane na tej drukarce; \
                 nie zostanie wydrukowane ponownie."
            )
    ));
    assert_eq!(sent(&sim, &RECEIPT_COMMANDS), frames);
    assert_eq!(sim.receipts().len(), 1);
    assert_eq!(history::load(&dir)?, history);
    assert_eq!(stored(&dir)?, printed, "the printed evidence is kept");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_claim_never_prints_over_another_unresolved_receipt() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let mut other = claimed(
        Device::Dispatching {
            stage: "trend".to_owned(),
        },
        None,
    )?;
    other.job_id = Uuid::from_u128(99);
    Journal::new(&dir).save(&Operation::Receipt(other.clone()))?;
    let mut agent = agent(&sim, &dir, &server)?;
    // `step` settles first; a claim racing an unresolved journal reaches `execute` directly.
    let job = agent.api.take()?.ok_or("missing claimed job")?;
    assert!(matches!(
        agent.execute(&job),
        Err(Error::Blocked(message)) if message == "Poprzednia operacja jest nierozstrzygnięta."
    ));
    assert_eq!(sent(&sim, &RECEIPT_COMMANDS), Vec::<Vec<u8>>::new());
    assert_eq!(stored(&dir)?, other);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn unnumbered_completion_remains_unknown_until_staff_supply_the_number() -> TestResult {
    let sim = Simulator::start();
    sim.fault("scnt", DeviceFault::Reply(Vec::new()));
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    let _delay = agent.step()?;
    assert_eq!(server.job(JOB_ID)["state"], "unknown");
    assert_eq!(
        stored(&dir)?.device,
        Device::Completed {
            receipt_number: None
        }
    );
    server.resolve(JOB_ID, Some("P41"))?;
    assert_eq!(agent.settle()?, None);
    assert_eq!(
        history::load(&dir)?
            .first()
            .ok_or("missing history")?
            .receipt_number
            .as_deref(),
        Some("P41")
    );
    assert_eq!(sim.receipts().len(), 1, "staff resolution never reprints");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn advance_allocation_and_named_mobile_payment_print_without_mutation() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let _id = server.enqueue(with(
        job_json(),
        &json!({
            "kind": "advance", "allocationId": ALLOCATION_ID,
            "total": "250.00", "payments": [{"form": "mobile", "amount": "250.00"}],
            "lines": [
                {"name": "Zaliczka 23%", "quantity": "1", "unitPrice": "200.00",
                 "total": "200.00", "vatRate": "23", "vatLetter": "A"},
                {"name": "Zaliczka 8%", "quantity": "1", "unitPrice": "50.00",
                 "total": "50.00", "vatRate": "8", "vatLetter": "B"}
            ]
        }),
    ));
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    let _delay = agent.step()?;
    let receipts = sim.receipts();
    let receipt = receipts.first().ok_or("missing advance receipt")?;
    assert_eq!(
        receipt.lines,
        [
            BTreeMap::from([
                ("na".to_owned(), "Zaliczka 23%".to_owned()),
                ("il".to_owned(), "1".to_owned()),
                ("vt".to_owned(), "0".to_owned()),
                ("pr".to_owned(), "20000".to_owned()),
                ("wa".to_owned(), "20000".to_owned()),
            ]),
            BTreeMap::from([
                ("na".to_owned(), "Zaliczka 8%".to_owned()),
                ("il".to_owned(), "1".to_owned()),
                ("vt".to_owned(), "1".to_owned()),
                ("pr".to_owned(), "5000".to_owned()),
                ("wa".to_owned(), "5000".to_owned()),
            ]),
        ],
        "advance quantities, VAT slots, prices and gross values reach the device unchanged"
    );
    assert_eq!(
        receipt.payments,
        [BTreeMap::from([
            ("ty".to_owned(), "6".to_owned()),
            ("na".to_owned(), "Płatność mobilna".to_owned()),
            ("wa".to_owned(), "25000".to_owned()),
        ])]
    );
    let requests = server.requests();
    let heartbeat = requests
        .iter()
        .find(|request| request.path == "/agent/session")
        .ok_or("missing VAT mapping evidence")?;
    assert_eq!(
        heartbeat.body["vatRates"],
        json!([
            {"letter": "A", "vatRate": "23"}, {"letter": "B", "vatRate": "8"},
            {"letter": "C", "vatRate": "5"}, {"letter": "D", "vatRate": "0"},
            {"letter": "E", "vatRate": "zw"}
        ])
    );
    assert_eq!(server.job(JOB_ID)["state"], "done");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn result_notes_are_canonical_nfc_before_journaling_and_sending() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(job_json());
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    let mut operation = claimed(Device::NotStarted, None)?;
    agent.report(&mut operation, JobResult::failed("  Cafe\u{301}  "))?;
    assert_eq!(
        stored(&dir)?.result,
        Some(JobResult::Failed {
            reason: "Café".to_owned()
        })
    );
    let requests = server.requests();
    assert_eq!(
        requests.last().ok_or("missing result request")?.body["result"],
        "Café"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn orphan_acknowledgement_rechecks_the_server_printer_binding() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(with(job_json(), &json!({"state": "failed"})));
    let (_temporary, dir) = state_dir();
    let operation = claimed(
        Device::Completed {
            receipt_number: Some("41".to_owned()),
        },
        None,
    )?;
    Journal::new(&dir).save(&Operation::Receipt(operation.clone()))?;
    let mut agent = agent(&sim, &dir, &server)?;
    assert_eq!(agent.settle()?, Some(Blocked::OrphanedReceipt));
    // Holuto answers 404 for another printer's job; script a misbound reply instead.
    let foreign = with(
        server.job(JOB_ID),
        &json!({"printerId": "00000000-0000-0000-0000-000000000099"}),
    );
    server.fault(
        "GET",
        &format!("/agent/jobs/{JOB_ID}"),
        Fault::Body {
            status: 200,
            body: serde_json::to_vec(&foreign)?,
        },
    );
    let expected = concat!(
        "Nie można uzgodnić niepotwierdzonego paragonu z Holuto: ",
        "zadanie innej drukarki. Drukowanie zablokowane."
    );
    assert!(
        matches!(agent.acknowledge_orphaned_receipt(), Err(Error::Blocked(message))
        if message == expected),
        "a changed server binding cannot erase the retained physical evidence"
    );
    assert_eq!(stored(&dir)?, operation);
    assert_eq!(sim.receipts(), []);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn orphan_acknowledgement_refuses_what_staff_or_settlement_must_decide() -> TestResult {
    let printed = Device::Completed {
        receipt_number: Some("41".to_owned()),
    };
    let dispatching = Device::Dispatching {
        stage: "trend".to_owned(),
    };
    let not_terminal = "Nie można uzgodnić niepotwierdzonego paragonu z Holuto: stan Holuto nie \
                        pozwala na lokalne wyjaśnienie. Drukowanie zablokowane.";
    let not_printed = "Brak wydrukowanego paragonu do wyjaśnienia.";
    for (remote, device, settled, expected) in [
        (json!({"state": "unknown"}), &printed, false, not_terminal),
        (json!({"state": "taken"}), &printed, false, not_terminal),
        (
            json!({"state": "failed", "attempt": Uuid::from_u128(98)}),
            &printed,
            false,
            not_terminal,
        ),
        (json!({"state": "failed"}), &printed, true, not_printed),
        (json!({"state": "failed"}), &dispatching, false, not_printed),
    ] {
        let sim = Simulator::start();
        let server = Holuto::new();
        server.insert(with(job_json(), &remote));
        let (_temporary, dir) = state_dir();
        let mut operation = claimed(device.clone(), None)?;
        if settled {
            operation.settled = Some(Settlement {
                at: "2025-01-02T12:01:00Z".parse()?,
                state: JobState::Failed,
            });
        }
        Journal::new(&dir).save(&Operation::Receipt(operation.clone()))?;
        let mut agent = agent(&sim, &dir, &server)?;
        let requests_before = server.requests().len();
        let refused = agent.acknowledge_orphaned_receipt();
        assert!(
            matches!(&refused, Err(Error::Blocked(message) | Error::Invalid(message))
                if message == expected),
            "{remote}, {device:?}, settled {settled}: {refused:?}"
        );
        assert_eq!(stored(&dir)?, operation, "{remote}: journal unchanged");
        assert!(
            server
                .requests()
                .iter()
                .skip(requests_before)
                .all(|request| request.method == "GET"),
            "{remote}: a refusal reports nothing to Holuto"
        );
        assert_eq!(sim.receipts(), [], "{remote}: a refusal never prints");
    }
    Ok(())
}

mod reconciliation;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn debug_diagnostics_do_not_disclose_receipt_customer_or_error_text() -> TestResult {
    let summary = Summary {
        total: Grosze::new(987_654)?,
        payments: Vec::new(),
        items: vec!["PrivateReceiptLine".to_owned()],
    };
    let entry = HistoryEntry {
        job_id: JOB_ID,
        printed_at: "2025-01-02T12:00:00Z".parse()?,
        receipt_number: Some("PrivateReceiptNumber".to_owned()),
        total: summary.total,
        payments: Vec::new(),
        items: summary.items.clone(),
    };
    let document: Document = serde_json::from_value(json!({
        "kind": "sale", "orderId": JOB_ID, "orderNumber": 10, "allocationId": null,
        "amount": "9876.54", "customerName": "PrivateCustomer",
        "occurredAt": "2025-01-02T12:00:00Z",
        "dueAt": "2025-01-02T12:00:00Z",
        "latestAt": "2025-01-02T12:00:00Z",
        "isOverdue": false
    }))?;
    let nip = BuyerNip::parse("5261040828")?;
    let result = JobResult::failed("PrivateResultReason");
    let error = Error::Invalid("PrivateErrorText".to_owned());
    let api_error = ApiError::Response("PrivateApiText".to_owned());
    let mut operation = claimed(
        Device::Completed {
            receipt_number: Some("PrivateReceiptNumber".to_owned()),
        },
        Some(JobResult::Done {
            receipt_number: "PrivateReceiptNumber".to_owned(),
        }),
    )?;
    operation.summary = Some(summary.clone());
    let debug = format!(
        "{summary:?}{entry:?} {document:?} {nip:?} {result:?} {error:?} {api_error:?} {operation:?}"
    );
    for forbidden in [
        "PrivateReceiptLine",
        "PrivateReceiptNumber",
        "987654",
        "9876.54",
        "PrivateCustomer",
        "5261040828",
        "PrivateResultReason",
        "PrivateErrorText",
        "PrivateApiText",
    ] {
        assert!(!debug.contains(forbidden), "Debug disclosed {forbidden}");
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove wrong-vendor evidence cannot be settled."
)]
fn resolution_requires_the_same_vendor_and_serial() -> TestResult {
    for (vendor, unique_number) in [(Vendor::Novitus, UNIQUE_NUMBER), (Vendor::Posnet, "OTHER")] {
        let sim = Simulator::start();
        let server = Holuto::new();
        server.insert(job_json());
        let (_temporary, dir) = state_dir();
        let mut operation = claimed(Device::NotStarted, None)?;
        operation.vendor = vendor;
        operation.device_unique_number = unique_number.to_owned();
        Journal::new(&dir).save(&Operation::Receipt(operation))?;
        let bytes = fs::read(dir.file("operation.json"))?;
        let mut agent = agent(&sim, &dir, &server)?;
        let requests = server.requests().len();
        let frames = sim.frames();
        let expected = format!(
            "Niepotwierdzony paragon dotyczy drukarki {vendor:?} {unique_number}. Podłącz ją, \
             aby uzgodnić wynik z Holuto."
        );
        assert!(matches!(agent.settle(), Err(Error::Blocked(message)) if message == expected));
        assert_eq!(server.requests().len(), requests);
        assert_eq!(fs::read(dir.file("operation.json"))?, bytes);
        assert_eq!(sim.frames(), frames);
        assert_eq!(sim.receipts(), []);
        assert_eq!(sim.reports(), []);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove wrong-device reports stay unresolved."
)]
fn report_resolution_requires_the_same_vendor_and_serial() -> TestResult {
    for (vendor, unique_number) in [(Vendor::Novitus, UNIQUE_NUMBER), (Vendor::Posnet, "OTHER")] {
        let sim = Simulator::start();
        let server = Holuto::new();
        let (_temporary, dir) = state_dir();
        Journal::new(&dir).save(&Operation::Report(ReportOperation {
            report: Report::Daily,
            vendor,
            device_unique_number: unique_number.to_owned(),
            started_at: "2025-01-02T12:00:00Z".parse()?,
            device: Device::Dispatching {
                stage: "dailyrep".to_owned(),
            },
            acknowledged_at: None,
        }))?;
        let agent = agent(&sim, &dir, &server)?;
        let bytes = fs::read(dir.file("operation.json"))?;
        let frames = sim.frames();
        let expected = format!(
            "Podłącz drukarkę {vendor:?} {unique_number}, której dotyczy przerwany raport."
        );
        assert!(
            matches!(agent.resolve_report(), Err(Error::Blocked(message)) if message == expected)
        );
        assert_eq!(fs::read(dir.file("operation.json"))?, bytes);
        assert_eq!(sim.frames(), frames);
        assert_eq!(agent.blocked()?, Some(Blocked::Report));
        assert_eq!(sim.reports(), []);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove wrong-device orphan evidence stays unresolved."
)]
fn orphan_acknowledgement_requires_the_same_vendor_and_serial() -> TestResult {
    for (vendor, unique_number) in [(Vendor::Novitus, UNIQUE_NUMBER), (Vendor::Posnet, "OTHER")] {
        let sim = Simulator::start();
        let server = Holuto::new();
        server.insert(with(job_json(), &json!({"state": "failed"})));
        let (_temporary, dir) = state_dir();
        let mut operation = claimed(
            Device::Completed {
                receipt_number: Some("41".to_owned()),
            },
            None,
        )?;
        operation.vendor = vendor;
        operation.device_unique_number = unique_number.to_owned();
        Journal::new(&dir).save(&Operation::Receipt(operation))?;
        let mut agent = agent(&sim, &dir, &server)?;
        let bytes = fs::read(dir.file("operation.json"))?;
        let frames = sim.frames();
        let requests = server.requests().len();
        let expected =
            format!("Podłącz drukarkę {vendor:?} {unique_number}, która wydrukowała ten paragon.");
        let result = agent.acknowledge_orphaned_receipt();
        assert!(matches!(result, Err(Error::Blocked(message)) if message == expected));
        assert_eq!(fs::read(dir.file("operation.json"))?, bytes);
        assert_eq!(sim.frames(), frames);
        assert_eq!(server.requests().len(), requests);
        assert_eq!(sim.receipts(), []);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove terminal recovery never reprints."
)]
fn voided_completed_receipt_settles_without_orphan_acknowledgement() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(with(job_json(), &json!({"state":"voided"})));
    let (_temporary, dir) = state_dir();
    Journal::new(&dir).save(&Operation::Receipt(claimed(
        Device::Completed {
            receipt_number: Some("41".to_owned()),
        },
        None,
    )?))?;
    let mut agent = agent(&sim, &dir, &server)?;
    assert_eq!(agent.settle()?, None);
    assert_eq!(
        stored(&dir)?.settled.as_ref().map(|value| value.state),
        Some(JobState::Voided)
    );
    assert!(
        server
            .requests()
            .iter()
            .all(|request| !request.path.ends_with("/result"))
    );
    assert_eq!(sim.receipts(), []);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove profile updates never print automatically."
)]
fn changed_vat_profile_uploads_once_and_illegal_letters_stay_local() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    let _delay = agent.step()?;
    let sessions = || {
        server
            .requests()
            .iter()
            .filter(|request| request.path == "/agent/session")
            .count()
    };
    assert_eq!(sessions(), 1);
    let _delay = agent.step()?;
    assert_eq!(sessions(), 1);
    sim.set_vat(["23,00", "8,00", "5,00", "0,00", "100,00", "22,00", "101,00"]);
    let _delay = agent.step()?;
    assert_eq!(sessions(), 2);
    sim.set_vat(["22,00", "8,00", "5,00", "0,00", "100,00", "22,00", "101,00"]);
    let _delay = agent.step()?;
    assert_eq!(sessions(), 2);
    assert!(agent.status.as_ref().is_some_and(|device| !device.ready()));
    assert!(
        server
            .requests()
            .iter()
            .filter(|request| request.path == "/agent/heartbeat")
            .all(|request| request.body.is_null())
    );
    assert_eq!(sim.receipts(), []);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_printer_swapped_mid_session_claims_no_job_and_prints_nothing() -> TestResult {
    const PAIRED: &str = "DEM0000000002";
    let sim = Simulator::start();
    let server = Holuto::new();
    let (_temporary, dir) = state_dir();
    let api = Api::new(server.url(), server.token(), PAIRED)?;
    let mut agent = Agent::new(sim.driver(), api, &dir, &waiting);
    // The session started on the paired printer; another one now answers on its port.
    let mut paired = agent.printer.probe()?;
    PAIRED.clone_into(&mut paired.unique_number);
    let _notices = agent.describe(paired)?;
    assert!(agent.session()?);
    let id = server.enqueue(job_json());
    assert!(matches!(
        agent.step(),
        Err(Error::Blocked(message))
            if message == format!(
                "Podłączona drukarka {UNIQUE_NUMBER} nie jest drukarką {PAIRED} sparowaną przy \
                 uruchomieniu. Podłącz ją ponownie albo zatrzymaj i uruchom obsługę."
            )
    ));
    assert!(
        server
            .requests()
            .iter()
            .all(|request| request.path != "/agent/jobs/take"),
        "no job is claimed for the paired printer"
    );
    assert_eq!(server.job(id)["state"], "queued");
    assert!(Journal::new(&dir).load()?.is_none());
    assert_eq!(sent(&sim, &RECEIPT_COMMANDS), Vec::<Vec<u8>>::new());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_claim_for_another_printer_or_without_an_attempt_is_never_printed() -> TestResult {
    for (changes, problem) in [
        (
            json!({"printerId": "00000000-0000-0000-0000-000000000099"}),
            "zadanie innej drukarki",
        ),
        (
            json!({"state": "queued"}),
            "zadanie nie zostało pobrane do wykonania",
        ),
        (
            json!({"attempt": null}),
            "zadanie nie zostało pobrane do wykonania",
        ),
    ] {
        let sim = Simulator::start();
        let server = Holuto::new();
        server.fault(
            "POST",
            "/agent/jobs/take",
            Fault::Body {
                status: 200,
                body: serde_json::to_vec(&with(job_json(), &changes))?,
            },
        );
        let (_temporary, dir) = state_dir();
        let mut agent = agent(&sim, &dir, &server)?;
        assert!(
            matches!(agent.step(), Err(Error::Blocked(message))
            if message == format!(
                "Nieprawidłowe zadanie {JOB_ID} z Holuto: {problem}. Nie drukuję go."
            )),
            "{changes}"
        );
        assert!(Journal::new(&dir).load()?.is_none(), "{changes}");
        assert!(
            server
                .requests()
                .iter()
                .all(|request| !request.path.ends_with("/result")),
            "a refused claim reports nothing for a job this printer does not own: {changes}"
        );
        assert_eq!(
            sent(&sim, &RECEIPT_COMMANDS),
            Vec::<Vec<u8>>::new(),
            "{changes}"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn journal_failures_after_print_preserve_the_last_durable_evidence_for_restart() -> TestResult {
    // Claim, trinit, two lines, payment and trend are six durable writes.
    for (successes, remote, device, result, blocked) in [
        (
            6,
            "taken",
            Device::Dispatching {
                stage: "trend".to_owned(),
            },
            None,
            true,
        ),
        (
            7,
            "taken",
            Device::Completed {
                receipt_number: Some("1".to_owned()),
            },
            None,
            false,
        ),
        (
            8,
            "done",
            Device::Completed {
                receipt_number: Some("1".to_owned()),
            },
            Some(JobResult::Done {
                receipt_number: "1".to_owned(),
            }),
            false,
        ),
    ] {
        let sim = Simulator::start();
        sim.set_clock("2025-01-02T12:34:00Z".parse()?);
        let server = Holuto::new();
        let id = server.enqueue(job_json());
        let (_temporary, dir) = state_dir();
        let mut first = agent(&sim, &dir, &server)?;
        first.journal.fail_write_after(successes);
        assert!(
            matches!(
                first.step(),
                Err(Error::Store { source, .. }) if source.kind() == ErrorKind::PermissionDenied
            ),
            "write after {successes}"
        );
        let before = stored(&dir)?;
        assert_eq!(before.device, device);
        assert_eq!(before.result, result);
        assert_eq!(before.settled, None);
        assert_eq!(server.job(id)["state"], remote);
        assert_eq!(sim.receipts().len(), 1);
        let frames = sent(&sim, &RECEIPT_COMMANDS);
        let results = server
            .requests()
            .into_iter()
            .filter(|request| request.path.ends_with("/result"))
            .map(|request| request.body)
            .collect::<Vec<_>>();
        let done = json!({
            "attempt": before.attempt, "state": "done", "receiptNumber": "1",
            "issuedAt": "2025-01-02T12:34:00Z", "result": null
        });
        assert_eq!(results, if successes == 8 { vec![done] } else { vec![] });
        drop(first);

        let mut recovered = agent(&sim, &dir, &server)?;
        assert_eq!(recovered.settle()?, blocked.then_some(Blocked::Receipt));
        let saved = stored(&dir)?;
        assert_eq!(saved.device, before.device);
        assert_eq!(saved.attempt, before.attempt);
        assert_eq!(
            saved.settled.as_ref().map(|settled| settled.state),
            if blocked { None } else { Some(JobState::Done) }
        );
        assert_eq!(
            server.job(id)["state"],
            if blocked { "unknown" } else { "done" }
        );
        if !blocked {
            assert_eq!(saved.issued_at.to_string(), "2025-01-02T12:34:00Z");
            let entries = history::load(&dir)?;
            assert_eq!(entries.len(), 1);
            let entry = entries.first().ok_or("missing recovered history")?;
            assert_eq!(entry.receipt_number.as_deref(), Some("1"));
            assert_eq!(entry.printed_at.to_string(), "2025-01-02T12:34:00Z");
        }
        assert_eq!(sim.receipts().len(), 1);
        assert_eq!(
            sent(&sim, &RECEIPT_COMMANDS),
            frames,
            "restart never reprints"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_failed_later_intent_write_keeps_the_previous_dispatch_blocked_after_restart() -> TestResult {
    for (successes, stage, expected) in [
        (2, "trinit", vec!["trinit"]),
        (
            5,
            "trpayment",
            vec!["trinit", "trline", "trline", "trpayment"],
        ),
    ] {
        let sim = Simulator::start();
        let server = Holuto::new();
        let id = server.enqueue(job_json());
        let (_temporary, dir) = state_dir();
        let mut first = agent(&sim, &dir, &server)?;
        first.journal.fail_write_after(successes);
        let _delay = first.step()?;
        let operation = stored(&dir)?;
        assert_eq!(
            operation.device,
            Device::Dispatching {
                stage: stage.to_owned()
            }
        );
        assert!(matches!(operation.result, Some(JobResult::Unknown { .. })));
        assert_eq!(operation.settled, None);
        assert_eq!(server.job(id)["state"], "unknown");
        let frames = sent(&sim, &RECEIPT_COMMANDS);
        let commands = frames
            .iter()
            .map(|frame| {
                let name = frame
                    .strip_prefix(&[0x02])
                    .ok_or("missing STX")?
                    .split(|byte| *byte == b'\t')
                    .next()
                    .ok_or("missing command")?;
                str::from_utf8(name).map_err(Into::<Box<dyn StdError>>::into)
            })
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(commands, expected);
        assert_eq!(sim.receipts(), []);
        drop(first);
        let mut recovered = agent(&sim, &dir, &server)?;
        assert_eq!(recovered.settle()?, Some(Blocked::Receipt));
        assert_eq!(stored(&dir)?, operation);
        assert_eq!(sent(&sim, &RECEIPT_COMMANDS), frames);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_report_completion_write_failure_requires_resolution_without_reprinting() -> TestResult {
    let sim = Simulator::start();
    sim.set_clock("2025-01-02T12:34:00Z".parse()?);
    let server = Holuto::new();
    let (_temporary, dir) = state_dir();
    let mut first = agent(&sim, &dir, &server)?;
    first.journal.fail_write_after(1);
    assert!(matches!(
        first.run_report(Report::Daily),
        Err(Error::Store { source, .. }) if source.kind() == ErrorKind::PermissionDenied
    ));
    let saved = Journal::new(&dir).load()?;
    let Some(Operation::Report(operation)) = &saved else {
        return Err("completed report must retain its dispatch intent".into());
    };
    assert_eq!(
        operation.device,
        Device::Dispatching {
            stage: "dailyrep".to_owned()
        }
    );
    assert_eq!(operation.acknowledged_at, None);
    let reports = sim.reports();
    assert_eq!(
        reports,
        [SimReport {
            command: "dailyrep".to_owned(),
            params: BTreeMap::from([("da".to_owned(), "2025-01-02".to_owned())]),
        }]
    );
    let frames = sent(&sim, &["dailyrep"]);
    drop(first);
    let recovered = agent(&sim, &dir, &server)?;
    assert_eq!(recovered.blocked()?, Some(Blocked::Report));
    recovered.resolve_report()?;
    assert_eq!(recovered.blocked()?, None);
    let resolved = fs::read(dir.file("operation.json"))?;
    assert!(matches!(
        recovered.resolve_report(),
        Err(Error::Invalid(message)) if message == "Brak przerwanego raportu do wyjaśnienia."
    ));
    assert_eq!(fs::read(dir.file("operation.json"))?, resolved);
    assert_eq!(sim.reports(), reports);
    assert_eq!(sent(&sim, &["dailyrep"]), frames);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_refusal_outbox_replays_while_taken_but_requires_staff_after_expiry() -> TestResult {
    for expired in [false, true] {
        let sim = Simulator::start();
        sim.fault("trinit", DeviceFault::Error);
        let server = Holuto::new();
        let id = server.enqueue(job_json());
        server.fault(
            "POST",
            &format!("/agent/jobs/{id}/result"),
            Fault::Problem {
                status: 503,
                code: "unavailable".to_owned(),
                detail: "temporary".to_owned(),
            },
        );
        let (_temporary, dir) = state_dir();
        let mut first = agent(&sim, &dir, &server)?;
        assert!(matches!(
            first.step(),
            Err(Error::Api(ApiError::Status { status: 503, .. }))
        ));
        let saved = stored(&dir)?;
        assert_eq!(
            saved.device,
            Device::Dispatching {
                stage: "trinit".to_owned()
            }
        );
        assert_eq!(
            saved.result,
            Some(JobResult::failed("Drukarka zwróciła błąd 2000 (trinit)."))
        );
        assert_eq!(server.job(id)["state"], "taken");
        let frames = sent(&sim, &RECEIPT_COMMANDS);
        if expired {
            server.expire(id);
        }
        drop(first);
        let mut recovered = agent(&sim, &dir, &server)?;
        assert_eq!(recovered.settle()?, expired.then_some(Blocked::Receipt));
        let results = server
            .requests()
            .into_iter()
            .filter(|request| request.path.ends_with("/result"))
            .map(|request| request.body)
            .collect::<Vec<_>>();
        let expected = json!({
            "attempt": saved.attempt, "state": "failed", "receiptNumber": null,
            "issuedAt": null, "result": "Drukarka zwróciła błąd 2000 (trinit)."
        });
        assert_eq!(
            results,
            if expired {
                vec![expected]
            } else {
                vec![expected.clone(), expected]
            }
        );
        if expired {
            assert_eq!(stored(&dir)?, saved);
            assert_eq!(server.job(id)["state"], "unknown");
            server.resolve(id, None)?;
            assert_eq!(recovered.settle()?, None);
        }
        assert_eq!(server.job(id)["state"], "failed");
        assert_eq!(
            stored(&dir)?.settled.map(|settled| settled.state),
            Some(JobState::Failed)
        );
        assert_eq!(sim.receipts(), []);
        assert_eq!(sent(&sim, &RECEIPT_COMMANDS), frames);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn an_uncertain_outbox_replays_after_http_failure_without_resending_the_print() -> TestResult {
    let sim = Simulator::start();
    sim.fault("trend", DeviceFault::LoseReply);
    let server = Holuto::new();
    let id = server.enqueue(job_json());
    server.fault(
        "POST",
        &format!("/agent/jobs/{id}/result"),
        Fault::Problem {
            status: 503,
            code: "unavailable".to_owned(),
            detail: "temporary".to_owned(),
        },
    );
    let (_temporary, dir) = state_dir();
    let mut first = agent(&sim, &dir, &server)?;
    assert!(matches!(
        first.step(),
        Err(Error::Api(ApiError::Status { status: 503, .. }))
    ));
    let saved = stored(&dir)?;
    assert_eq!(
        saved.device,
        Device::Dispatching {
            stage: "trend".to_owned()
        }
    );
    assert!(matches!(saved.result, Some(JobResult::Unknown { .. })));
    assert_eq!(server.job(id)["state"], "taken");
    let frames = sent(&sim, &RECEIPT_COMMANDS);
    drop(first);
    let mut recovered = agent(&sim, &dir, &server)?;
    assert_eq!(recovered.settle()?, Some(Blocked::Receipt));
    assert_eq!(server.job(id)["state"], "unknown");
    assert_eq!(stored(&dir)?, saved);
    let results = server
        .requests()
        .into_iter()
        .filter(|request| request.path.ends_with("/result"))
        .map(|request| request.body)
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert_eq!(
        results.first(),
        results.last(),
        "only the identical unknown result is replayed"
    );
    assert_eq!(sim.receipts().len(), 1);
    assert_eq!(sent(&sim, &RECEIPT_COMMANDS), frames);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_changed_vat_warning_survives_a_failed_poll() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    sim.set_vat(["23,00", "8,00", "5,00", "0,00", "100,00", "7,00", "101,00"]);
    server.fault(
        "POST",
        "/agent/heartbeat",
        Fault::Problem {
            status: 503,
            code: "unavailable".to_owned(),
            detail: "temporary".to_owned(),
        },
    );
    assert!(matches!(
        agent.step(),
        Err(Error::Api(ApiError::Status { status: 503, .. }))
    ));
    let _delay = agent.step()?;
    let mut notices = agent.take_news().notices;
    notices.retain(|notice| notice.starts_with("Pomijam"));
    assert_eq!(
        notices,
        ["Pomijam nieobsługiwaną stawkę VAT w slocie F · 7%"],
        "the warning for a changed VAT table is shown once, even when that poll fails"
    );
    Ok(())
}
