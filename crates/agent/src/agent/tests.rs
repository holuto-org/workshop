use std::{collections::BTreeMap, error::Error as StdError, fs, io::ErrorKind};

use fiscal::{device::Vendor, error::Error as PrinterError, money::Grosze, report::Report};
use posnet::{
    capabilities::capabilities,
    simulator::{Fault as DeviceFault, Simulator, UNIQUE_NUMBER},
};
use serde_json::json;

use super::{Agent, Blocked};
use crate::{
    Error,
    api::{Api, ApiError, model::JobState},
    documents::{BuyerNip, Document},
    history::{self, HistoryEntry},
    journal::{Device, JobResult, Journal, Operation, ReceiptOperation, ReportOperation, Summary},
    store::StateDir,
    testing::{ALLOCATION_ID, ATTEMPT, Fault, Holuto, JOB_ID, job_json, state_dir, with},
};

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
fn not_started_crash_reports_failed_without_blocking() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(job_json());
    let (_temporary, dir) = state_dir();
    Journal::new(&dir).save(&Operation::Receipt(claimed(Device::NotStarted, None)?))?;
    let mut agent = agent(&sim, &dir, &server)?;
    assert_eq!(agent.settle()?, None);
    assert_eq!(server.job(JOB_ID)["state"], "failed");
    assert_eq!(stored(&dir)?.device, Device::NotStarted);
    assert!(
        stored(&dir)?.settled.is_some(),
        "no dispatch proof settles safely"
    );
    assert!(sim.receipts().is_empty(), "recovery never prints");
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
    assert!(
        agent
            .take_news()
            .notices
            .iter()
            .any(|notice| notice.contains("nie wysłano żadnego polecenia fiskalnego")),
        "staff need the journal's certain evidence"
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
fn completed_crash_recovers_done_and_preserves_printer_issue_time() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(job_json());
    let (_temporary, dir) = state_dir();
    let operation = claimed(
        Device::Completed {
            receipt_number: Some("41".to_owned()),
        },
        None,
    )?;
    Journal::new(&dir).save(&Operation::Receipt(operation.clone()))?;
    let mut agent = agent(&sim, &dir, &server)?;
    assert_eq!(agent.settle()?, None);
    assert_eq!(server.job(JOB_ID)["receiptNumber"], "41");
    let reported: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|request| request.path.ends_with("/result"))
        .map(|request| request.body["issuedAt"].clone())
        .collect();
    assert_eq!(
        reported,
        [json!("2025-01-02T12:00:00Z")],
        "recovery reports the journaled printer time, not the recovery time"
    );
    assert!(
        sim.receipts().is_empty(),
        "recovery records evidence without printing"
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
fn dispatching_crash_stays_unknown_until_staff_resolution() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    server.insert(job_json());
    let (_temporary, dir) = state_dir();
    Journal::new(&dir).save(&Operation::Receipt(claimed(
        Device::Dispatching {
            stage: "trend".to_owned(),
        },
        None,
    )?))?;
    let mut agent = agent(&sim, &dir, &server)?;
    assert_eq!(agent.settle()?, Some(Blocked::Receipt));
    assert_eq!(server.job(JOB_ID)["state"], "unknown");
    server.resolve(JOB_ID, Some("P123"))?;
    assert_eq!(agent.settle()?, None);
    assert!(sim.receipts().is_empty(), "human resolution prints nothing");
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
        assert!(
            agent
                .take_news()
                .notices
                .iter()
                .any(|notice| notice.contains("Paragon 41")
                    && notice.contains("zlecenia 10")
                    && notice.contains(&JOB_ID.to_string())),
            "operator gets job, order and physical receipt evidence"
        );
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
    assert!(
        matches!(agent.settle(), Err(Error::Blocked(message)) if message.contains("inną próbę")),
        "attempt mismatch fails closed"
    );
    assert_eq!(stored(&dir)?, operation);
    assert!(sim.receipts().is_empty(), "attempt mismatch never prints");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn telemetry_reports_device_vendor_capabilities_and_precision() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    let _delay = agent.step()?;
    for request in server
        .requests()
        .iter()
        .filter(|request| request.path == "/agent/session")
    {
        assert_eq!(request.body["vendor"], "posnet");
        assert_eq!(request.body["capabilities"]["maxQuantityDecimals"], 8);
        assert_eq!(
            request.body["capabilities"],
            serde_json::to_value(&agent.telemetry.capabilities)?
        );
    }
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
        assert!(sim.frames().iter().all(|frame| {
            [
                "trinit",
                "trnipset",
                "trline",
                "tradvance",
                "trpayment",
                "trend",
                "trftrln",
                "trftrend",
            ]
            .iter()
            .all(|command| {
                !frame
                    .windows(command.len())
                    .any(|part| part == command.as_bytes())
            })
        }));
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn oversized_job_is_rejected_as_invalid_without_journal_or_result() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let line = json!({
        "name": "Pozycja", "quantity": "1", "unitPrice": "0.01",
        "total": "0.01", "vatRate": "23", "vatLetter": "A"
    });
    let changes = json!({"lines": vec![line; 501], "total": "5.01"});
    let _id = server.enqueue(with(job_json(), &changes));
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, &server)?;
    let outcome = agent.step();
    assert!(
        matches!(&outcome, Err(Error::Blocked(message)) if *message == format!(
            "Nieprawidłowe zadanie {JOB_ID} z Holuto: zadanie musi mieć od 1 do 500 pozycji. \
             Nie drukuję go."
        )),
        "{outcome:?}"
    );
    assert!(
        !dir.file("operation.json").exists(),
        "an invalid job is never journaled"
    );
    assert!(
        server
            .requests()
            .iter()
            .all(|request| !request.path.ends_with("/result")),
        "no terminal result is sent for an invalid job"
    );
    assert!(sim.receipts().is_empty(), "invalid job never prints");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn invalid_money_vat_or_text_fails_before_fiscal_commands() -> TestResult {
    for changes in [
        json!({"total":"249.99"}),
        json!({"lines": [{
            "name": "Pozycja", "quantity": "1", "unitPrice": "250.001",
            "total": "250.00", "vatRate": "23", "vatLetter": "A"
        }]}),
        json!({"lines": [{
            "name": "Pozycja", "quantity": "1", "unitPrice": "250.00",
            "total": "250.00", "vatRate": "8", "vatLetter": "A"
        }]}),
        json!({"companyName":"Firma 🛠"}),
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
    assert!(
        matches!(agent.step(), Err(Error::Blocked(_))),
        "corrupt journal blocks receipt"
    );
    assert!(
        matches!(agent.run_report(Report::Daily), Err(Error::Blocked(_))),
        "corrupt journal blocks reports"
    );
    assert!(
        sim.receipts().is_empty() && sim.reports().is_empty(),
        "corrupt intent cannot reach device"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn interrupted_report_is_acknowledged_without_reissuing_it() -> TestResult {
    let sim = Simulator::start();
    let server = Holuto::new();
    let (_temporary, dir) = state_dir();
    let operation = ReportOperation {
        report: Report::Daily,
        vendor: Vendor::Posnet,
        device_unique_number: UNIQUE_NUMBER.to_owned(),
        started_at: "2025-01-02T12:00:00Z".parse()?,
        device: Device::Dispatching {
            stage: "dailyrep".to_owned(),
        },
        acknowledged_at: None,
    };
    Journal::new(&dir).save(&Operation::Report(operation))?;
    let agent = agent(&sim, &dir, &server)?;
    agent.resolve_report()?;
    assert_eq!(agent.blocked()?, None);
    assert!(
        sim.reports().is_empty(),
        "resolution never prints the report again"
    );
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
    assert!(
        stored(&dir)?.settled.is_some(),
        "history is only a display cache"
    );
    assert!(
        agent
            .take_news()
            .notices
            .iter()
            .any(|notice| notice.contains("histor")),
        "operator sees history failure"
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
    agent.journal.fail_next_write();
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
    let fiscal = sim
        .frames()
        .into_iter()
        .filter(|frame| {
            [
                "trinit",
                "trnipset",
                "trline",
                "tradvance",
                "trpayment",
                "trend",
                "trftrln",
                "trftrend",
            ]
            .iter()
            .any(|command| {
                frame
                    .windows(command.len())
                    .any(|part| part == command.as_bytes())
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        fiscal,
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
