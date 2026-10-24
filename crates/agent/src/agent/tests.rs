//! Receipt execution and durable recovery against the socket simulator.

use std::{fs, thread, time::Duration};

use jiff::{Timestamp, civil::date};
use posnet::{
    printer::{Printer, Report},
    session::Connection,
    simulator::{Fault, SERIAL, Simulator},
};
use serde_json::{Value, json};

use super::{Agent, Blocked};
use crate::{
    Error,
    api::model::{JobState, Outcome, ResultForm},
    history,
    journal::{Device, JobResult, Journal, Operation, ReceiptOperation, ReportOperation},
    store::StateDir,
    testing::{
        ALLOCATION_ID, ATTEMPT, FakeApi, JOB_ID, PRINTER_ID, job, job_json, job_with, state_dir,
    },
};

fn agent<'dir>(sim: &Simulator, dir: &'dir StateDir, api: FakeApi) -> Agent<'dir, FakeApi> {
    let connection = Connection {
        port: sim.address().to_owned(),
        baud_rate: 9600,
    };
    Agent::new(Printer::new(sim.link(), connection), api, dir)
}

/// The same journal and Holuto after a process restart.
fn restart<'dir>(
    sim: &Simulator,
    dir: &'dir StateDir,
    previous: Agent<'_, FakeApi>,
) -> Agent<'dir, FakeApi> {
    agent(sim, dir, previous.api)
}

/// One poll whose outcome the test checks through the journal, the printer and Holuto.
fn poll(agent: &mut Agent<'_, FakeApi>) {
    let _step = agent.step().unwrap();
}

fn receipt_operation(dir: &StateDir) -> ReceiptOperation {
    match Journal::new(dir).load().unwrap() {
        Some(Operation::Receipt(operation)) => operation,
        other => panic!("expected a receipt operation, got {other:?}"),
    }
}

fn journal(dir: &StateDir, operation: ReceiptOperation) {
    Journal::new(dir)
        .save(&Operation::Receipt(operation))
        .unwrap();
}

/// A claimed job in the given device state, as a crash would leave it.
fn claimed(device: Device, result: Option<JobResult>) -> ReceiptOperation {
    ReceiptOperation {
        job_id: JOB_ID,
        attempt: ATTEMPT,
        issued_at: "2026-10-16T10:01:30+02:00".parse().unwrap(),
        device_serial: SERIAL.to_owned(),
        device,
        summary: None,
        result,
        settled: None,
    }
}

fn line(changes: &Value) -> Value {
    let mut line = job_json()["lines"][0].clone();
    for (key, value) in changes.as_object().unwrap() {
        line[key] = value.clone();
    }
    line
}

#[test]
fn prints_once_with_brand_footer_and_receipt_number() {
    let sim = Simulator::start();
    sim.set_header("&c&1Firma Inna&1&c\nul. Testowa 1");
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    let step = agent.step().unwrap();
    assert!(step.history_changed);
    let [receipt] = sim.receipts().try_into().unwrap();
    assert_eq!(receipt.total, 25_000);
    assert_eq!(
        receipt
            .lines
            .iter()
            .map(|line| line["vt"].as_str())
            .collect::<Vec<_>>(),
        ["0", "1"]
    );
    assert_eq!(sim.footers()[0]["na"], "Warsztat Kowalski");
    let operation = receipt_operation(&dir);
    assert_eq!(
        agent.api.reports,
        [ResultForm {
            attempt: ATTEMPT,
            state: Outcome::Done,
            receipt_number: Some("1".to_owned()),
            issued_at: Some(operation.issued_at),
            result: None,
        }]
    );
    assert_eq!(
        operation.settled.map(|settled| settled.state),
        Some(JobState::Done)
    );
    assert_eq!(agent.api.heartbeats.len(), 1);
    assert_eq!(agent.blocked().unwrap(), None);
    let [entry] = history::load(&dir).unwrap().try_into().unwrap();
    assert_eq!(
        (entry.receipt_number.as_deref(), entry.total.get()),
        (Some("1"), 25_000)
    );
    assert_eq!(entry.items, ["Wymiana oleju", "Klocki hamulcowe"]);
}

#[test]
fn a_lost_result_is_resent_unchanged_after_restart() {
    for remote_state in [JobState::Taken, JobState::Unknown] {
        let sim = Simulator::start();
        let (_temporary, dir) = state_dir();
        let mut api = FakeApi::new(vec![job()]);
        api.fail_report = true;
        let mut first = agent(&sim, &dir, api);
        assert!(matches!(first.step(), Err(Error::Api(_))));
        let original = first.api.reports[0].clone();
        first.api.fail_report = false;
        first.api.remote().state = remote_state;
        let mut second = restart(&sim, &dir, first);
        poll(&mut second);
        assert_eq!(second.api.reports, [original.clone(), original]);
        assert_eq!(sim.receipts().len(), 1);
        assert!(receipt_operation(&dir).settled.is_some());
    }
}

#[test]
fn a_crash_after_printing_reports_done_with_the_journaled_issue_time() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let operation = claimed(
        Device::Completed {
            receipt_number: Some("41".to_owned()),
        },
        None,
    );
    journal(&dir, operation.clone());
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![]));
    poll(&mut agent);
    assert_eq!(agent.api.reports[0].state, Outcome::Done);
    assert_eq!(agent.api.reports[0].receipt_number.as_deref(), Some("41"));
    assert_eq!(agent.api.reports[0].issued_at, Some(operation.issued_at));
    assert_eq!(sim.receipts().len(), 0);
}

#[test]
fn a_crash_after_the_claim_is_unknown_and_never_replayed() {
    for device in [
        Device::NotStarted,
        Device::Dispatching {
            stage: "trline".to_owned(),
        },
    ] {
        let sim = Simulator::start();
        let (_temporary, dir) = state_dir();
        journal(&dir, claimed(device, None));
        let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
        let step = agent.step().unwrap();
        assert_eq!(step.delay, super::RETRY);
        assert_eq!(agent.api.reports[0].state, Outcome::Unknown);
        assert_eq!(agent.api.jobs.len(), 1, "no new job while unresolved");
        assert_eq!(sim.receipts().len(), 0);
        assert_eq!(agent.blocked().unwrap(), Some(Blocked::Receipt));
    }
}

#[test]
fn an_unknown_result_is_stable_and_never_automatically_failed() {
    let sim = Simulator::start();
    sim.fault("trend", Fault::Error);
    let (_temporary, dir) = state_dir();
    let mut api = FakeApi::new(vec![job(), job()]);
    api.fail_report = true;
    let mut first = agent(&sim, &dir, api);
    assert!(first.step().is_err());
    let original = first.api.reports[0].clone();
    assert_eq!(
        (original.state, original.issued_at),
        (Outcome::Unknown, None)
    );
    first.api.fail_report = false;
    let mut second = restart(&sim, &dir, first);
    poll(&mut second);
    poll(&mut second);
    assert_eq!(second.api.reports, [original.clone(), original]);
    assert_eq!(second.api.jobs.len(), 1);
    assert_eq!(second.api.heartbeats.len(), 3);
    assert_eq!(sim.receipts().len(), 0);
}

#[test]
fn a_terminal_server_state_settles_without_printing_or_reporting_again() {
    for terminal in [JobState::Done, JobState::Failed, JobState::Expired] {
        let sim = Simulator::start();
        let (_temporary, dir) = state_dir();
        journal(
            &dir,
            claimed(
                Device::Dispatching {
                    stage: "trend".to_owned(),
                },
                Some(JobResult::unknown("x")),
            ),
        );
        let mut api = FakeApi::new(vec![]);
        api.remote().state = terminal;
        api.remote().result = Some("Resolved by staff".to_owned());
        let mut agent = agent(&sim, &dir, api);
        poll(&mut agent);
        poll(&mut agent);
        assert_eq!(agent.api.reports, []);
        assert_eq!(
            receipt_operation(&dir).settled.map(|settled| settled.state),
            Some(terminal)
        );
        assert_eq!(agent.blocked().unwrap(), None);
        assert_eq!(sim.receipts().len(), 0);
    }
}

#[test]
fn malformed_or_unowned_jobs_never_reach_the_printer() {
    for changes in [
        json!({"state": "done"}),
        json!({"attempt": null}),
        json!({"printerId": JOB_ID}),
        json!({"kind": "advance"}),
        json!({"lines": []}),
        json!({"buyerNip": "123"}),
        json!({"orderNumber": 0}),
    ] {
        let sim = Simulator::start();
        let (_temporary, dir) = state_dir();
        let mut agent = agent(&sim, &dir, FakeApi::new(vec![job_with(&changes)]));
        assert!(matches!(agent.step(), Err(Error::Blocked(_))), "{changes}");
        assert_eq!(sim.receipts().len(), 0);
        assert_eq!(Journal::new(&dir).load().unwrap(), None, "{changes}");
    }
}

#[test]
fn an_invalid_amount_fails_the_job_before_printing_and_resends_the_same_result() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let mut api = FakeApi::new(vec![job_with(
        &json!({"lines": [line(&json!({"quantity": "NaN"}))]}),
    )]);
    api.fail_report = true;
    let mut first = agent(&sim, &dir, api);
    assert!(first.step().is_err());
    first.api.fail_report = false;
    let mut second = restart(&sim, &dir, first);
    poll(&mut second);
    assert_eq!(second.api.reports[0].state, Outcome::Failed);
    assert_eq!(second.api.reports[0], second.api.reports[1]);
    assert_eq!(sim.receipts().len(), 0);
}

#[test]
fn a_failed_result_is_never_resent_over_a_server_unknown() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    journal(
        &dir,
        claimed(
            Device::NotStarted,
            Some(JobResult::failed("Nieprawidłowa liczba")),
        ),
    );
    let mut api = FakeApi::new(vec![]);
    api.remote().state = JobState::Unknown;
    let mut agent = agent(&sim, &dir, api);
    poll(&mut agent);
    assert_eq!(agent.api.reports, []);
    assert_eq!(agent.blocked().unwrap(), Some(Blocked::Receipt));
}

#[test]
fn an_unready_printer_sends_fresh_telemetry_without_claiming() {
    let sim = Simulator::start();
    sim.fault(
        "sprn",
        Fault::Reply(vec![("pr".to_owned(), "5".to_owned())]),
    );
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    let step = agent.step().unwrap();
    assert!(
        step.notices
            .iter()
            .any(|notice| notice.contains("Brak papieru")),
        "{:?}",
        step.notices
    );
    assert_eq!(agent.api.jobs.len(), 1);
    assert_eq!(agent.api.heartbeats[0].model, "POSNET TEMO ONLINE");
    assert_eq!(sim.receipts().len(), 0);
}

#[test]
fn a_failed_probe_still_sends_the_latest_heartbeat_and_takes_nothing() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    let status = agent.printer.probe().unwrap();
    let _notices = agent.describe(status).unwrap();
    sim.fault("getrealid", Fault::Error);
    assert!(matches!(agent.step(), Err(Error::Printer(_))));
    assert_eq!(agent.api.heartbeats[0].firmware, "32.01");
    assert_eq!(agent.api.jobs.len(), 1);
}

#[test]
fn a_swapped_device_is_refused_before_a_claim() {
    let sim = Simulator::start();
    sim.fault(
        "getrealid",
        Fault::Reply(vec![
            ("nm".to_owned(), "POSNET TEMO ONLINE".to_owned()),
            ("vr".to_owned(), "32.01".to_owned()),
            ("nu".to_owned(), "OTHER".to_owned()),
        ]),
    );
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    let error = agent.step().unwrap_err();
    assert!(error.to_string().contains("OTHER"), "{error}");
    assert_eq!(agent.api.jobs.len(), 1);
    assert_eq!(sim.receipts().len(), 0);
}

#[test]
fn telemetry_reports_the_device_slot_table() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![]));
    let status = agent.printer.probe().unwrap();
    assert_eq!(agent.describe(status).unwrap(), Vec::<String>::new());
    let telemetry = serde_json::to_value(&agent.telemetry).unwrap();
    assert_eq!(
        telemetry["vatSlots"],
        json!([
            {"slot": 0, "vatRate": "23"},
            {"slot": 1, "vatRate": "8"},
            {"slot": 2, "vatRate": "0"},
            {"slot": 6, "vatRate": "zw"}
        ])
    );
    // `Telemetry` in the backend's openapi/agent.json: all required, no other properties.
    let mut fields: Vec<&str> = telemetry
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "allowedCharacters",
            "appVersion",
            "firmware",
            "maxCompanyNameLength",
            "maxItemNameLength",
            "maxLinesPerReceipt",
            "model",
            "serial",
            "vatSlots"
        ]
    );
    assert_eq!(
        (
            telemetry["maxItemNameLength"].as_u64(),
            telemetry["maxLinesPerReceipt"].as_u64(),
            telemetry["maxCompanyNameLength"].as_u64()
        ),
        (Some(80), Some(500), Some(40))
    );
    assert_eq!(telemetry["serial"], SERIAL);
}

#[test]
fn an_unmappable_active_rate_is_omitted_with_a_notice() {
    let sim = Simulator::start();
    sim.set_vat([
        "22,00", "5,00", "101,00", "101,00", "101,00", "101,00", "101,00",
    ]);
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![]));
    let status = agent.printer.probe().unwrap();
    let notices = agent.describe(status).unwrap();
    assert_eq!(
        serde_json::to_value(&agent.telemetry.vat_slots).unwrap(),
        json!([{"slot": 1, "vatRate": "5"}])
    );
    assert!(
        notices.iter().any(|notice| notice.contains("A · 22%")),
        "{notices:?}"
    );
}

#[test]
fn an_exempt_workshop_prints_on_the_exempt_slot() {
    let sim = Simulator::start();
    sim.set_vat([
        "101,00", "101,00", "101,00", "101,00", "101,00", "101,00", "100,00",
    ]);
    let (_temporary, dir) = state_dir();
    let lines = json!([
        line(&json!({"vatRate": "zw", "vatSlot": 6})),
        line(
            &json!({"name": "Klocki", "quantity": "1", "unitPrice": "50.00", "total": "50.00", "vatRate": "zw", "vatSlot": 6})
        )
    ]);
    let mut agent = agent(
        &sim,
        &dir,
        FakeApi::new(vec![job_with(&json!({"lines": lines}))]),
    );
    poll(&mut agent);
    assert_eq!(agent.api.reports[0].state, Outcome::Done);
    assert!(sim.receipts()[0].lines.iter().all(|line| line["vt"] == "6"));
}

#[test]
fn a_corrupt_journal_blocks_printing() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    fs::write(dir.path().join("operation.json"), "not json").unwrap();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    assert!(matches!(agent.step(), Err(Error::Blocked(message)) if message.contains("Uszkodzony")));
    assert!(agent.blocked().is_err());
    assert_eq!(agent.api.jobs.len(), 1);
    assert_eq!(sim.receipts().len(), 0);
}

#[test]
fn a_missing_receipt_number_stays_unknown_until_staff_resolve_it() {
    let sim = Simulator::start();
    sim.fault("scnt", Fault::Error);
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    poll(&mut agent);
    poll(&mut agent);
    assert_eq!(agent.api.reports.len(), 1);
    let report = &agent.api.reports[0];
    assert_eq!(
        (
            report.state,
            report.receipt_number.as_deref(),
            report.issued_at
        ),
        (Outcome::Unknown, None, None)
    );
    assert!(
        report
            .result
            .as_deref()
            .is_some_and(|result| result.contains("numeru"))
    );
    assert_eq!(
        receipt_operation(&dir).device,
        Device::Completed {
            receipt_number: None
        }
    );
    assert_eq!(agent.blocked().unwrap(), Some(Blocked::Receipt));

    agent.api.remote().state = JobState::Done;
    agent.api.remote().receipt_number = Some("staff-number".to_owned());
    poll(&mut agent);
    assert_eq!(agent.blocked().unwrap(), None);
    assert_eq!(sim.receipts().len(), 1);
    assert_eq!(
        history::load(&dir).unwrap()[0].receipt_number.as_deref(),
        Some("staff-number")
    );
}

#[test]
fn completed_evidence_upgrades_an_unknown_result_to_done() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let operation = claimed(
        Device::Completed {
            receipt_number: Some("12".to_owned()),
        },
        Some(JobResult::unknown("lost reply")),
    );
    journal(&dir, operation.clone());
    let mut api = FakeApi::new(vec![]);
    api.remote().state = JobState::Unknown;
    let mut agent = agent(&sim, &dir, api);
    poll(&mut agent);
    assert_eq!(agent.api.reports[0].state, Outcome::Done);
    assert_eq!(agent.api.reports[0].issued_at, Some(operation.issued_at));
    assert_eq!(agent.blocked().unwrap(), None);
    assert_eq!(sim.receipts().len(), 0);
}

#[test]
fn a_different_server_attempt_keeps_the_journal_and_blocks() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    journal(
        &dir,
        claimed(
            Device::Completed {
                receipt_number: Some("1".to_owned()),
            },
            None,
        ),
    );
    let mut api = FakeApi::new(vec![]);
    api.remote().attempt = Some(PRINTER_ID);
    let mut agent = agent(&sim, &dir, api);
    assert!(matches!(agent.step(), Err(Error::Blocked(_))));
    assert_eq!(agent.api.reports, []);
    assert!(receipt_operation(&dir).settled.is_none());
}

#[test]
fn a_repeated_claim_of_the_same_job_is_never_printed_again() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job(), job()]));
    poll(&mut agent);
    assert!(
        matches!(agent.step(), Err(Error::Blocked(message)) if message.contains("nie zostanie wydrukowane"))
    );
    assert_eq!(sim.receipts().len(), 1);
}

#[test]
fn a_device_swap_after_the_claim_is_unknown_and_prints_nothing() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let hold = sim.hold("vatget");
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    thread::scope(|scope| {
        let worker = scope.spawn(|| agent.step());
        // The step's own probe passes; swap the device before the execution probe.
        assert!(hold.reached(Duration::from_secs(5)));
        sim.fault(
            "getrealid",
            Fault::Reply(vec![
                ("nm".to_owned(), "POSNET TEMO ONLINE".to_owned()),
                ("vr".to_owned(), "32.01".to_owned()),
                ("nu".to_owned(), "OTHER".to_owned()),
            ]),
        );
        hold.release();
        assert!(worker.join().unwrap().is_err());
    });
    assert_eq!(receipt_operation(&dir).device, Device::NotStarted);
    assert_eq!(sim.receipts().len(), 0);
    sim.clear_fault("getrealid");
    let mut agent = restart(&sim, &dir, agent);
    poll(&mut agent);
    assert_eq!(agent.api.reports[0].state, Outcome::Unknown);
}

#[cfg(unix)]
#[test]
fn a_claim_that_cannot_be_journaled_is_never_executed() {
    use std::os::unix::fs::PermissionsExt as _;

    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    let result = agent.step();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(result, Err(Error::Store { .. })), "{result:?}");
    assert_eq!(agent.api.jobs.len(), 0, "the job was taken");
    assert_eq!(Journal::new(&dir).load().unwrap(), None);
    assert!(
        !sim.frames()
            .iter()
            .any(|frame| frame.windows(6).any(|window| window == b"trinit"))
    );
    let mut agent = restart(&sim, &dir, agent);
    poll(&mut agent);
    assert_eq!(
        sim.receipts().len(),
        0,
        "a taken job whose claim was lost is never printed"
    );
}

#[test]
fn an_unresolved_receipt_blocks_reports_and_cannot_be_resolved_locally() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    journal(
        &dir,
        claimed(
            Device::Dispatching {
                stage: "trend".to_owned(),
            },
            Some(JobResult::unknown("x")),
        ),
    );
    let mut api = FakeApi::new(vec![]);
    api.remote().state = JobState::Unknown;
    let mut agent = agent(&sim, &dir, api);
    assert!(matches!(
        agent.run_report(Report::Daily),
        Err(Error::Blocked(_))
    ));
    assert!(
        matches!(agent.resolve_report(), Err(Error::Invalid(message)) if message.contains("Holuto"))
    );
    assert_eq!(sim.reports().len(), 0);
    assert_eq!(agent.blocked().unwrap(), Some(Blocked::Receipt));
}

#[test]
fn an_advance_job_prints_one_line_per_rate_with_a_named_mobile_payment() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let name = "Zaliczka – naprawa pojazdu, zlecenie 10";
    let advance = job_with(&json!({
        "kind": "advance",
        "allocationId": ALLOCATION_ID,
        "payments": [{"form": "mobile", "amount": "250.00"}],
        "lines": [
            line(&json!({"name": name, "quantity": "1", "unitPrice": "200.00", "total": "200.00"})),
            line(&json!({
                "name": name, "quantity": "1", "unitPrice": "50.00", "total": "50.00",
                "vatRate": "8", "vatSlot": 1
            }))
        ]
    }));
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![advance]));
    poll(&mut agent);
    assert_eq!(agent.api.reports[0].state, Outcome::Done);
    let [receipt] = sim.receipts().try_into().unwrap();
    assert_eq!(receipt.total, 25_000);
    let lines: Vec<_> = receipt
        .lines
        .iter()
        .map(|line| {
            (
                line["vt"].as_str(),
                line["il"].as_str(),
                line["pr"].as_str(),
            )
        })
        .collect();
    assert_eq!(lines, [("0", "1", "20000"), ("1", "1", "5000")]);
    assert_eq!(receipt.payments[0]["na"], "Płatność mobilna");
}

#[test]
fn settlement_with_advances_and_credit_reaches_the_printer() {
    let cases = [
        (
            json!([{"vatSlot": 0, "vatRate": "23", "amount": "100.00"}]),
            json!([{"form": "card", "amount": "50.00"}, {"form": "credit", "amount": "100.00"}]),
            15_000,
        ),
        (
            json!([{"vatSlot": 0, "vatRate": "23", "amount": "200.00"}, {"vatSlot": 1, "vatRate": "8", "amount": "50.00"}]),
            json!([]),
            0,
        ),
    ];
    for (advances, payments, fiscal) in cases {
        let sim = Simulator::start();
        let (_temporary, dir) = state_dir();
        let mut agent = agent(
            &sim,
            &dir,
            FakeApi::new(vec![job_with(
                &json!({"advances": advances, "payments": payments}),
            )]),
        );
        poll(&mut agent);
        assert_eq!(
            agent.api.reports[0].state,
            Outcome::Done,
            "{:?}",
            agent.api.reports
        );
        assert_eq!(sim.receipts()[0].total, fiscal);
    }
}

#[test]
fn an_invalid_job_fails_without_fiscal_commands() {
    let one = |slot: u8, rate: &str, amount: &str| json!({"vatSlot": slot, "vatRate": rate, "amount": amount});
    let cash = |amount: &str| json!([{"form": "cash", "amount": amount}]);
    let cases = [
        json!({"advances": [one(0, "23", "200.01")]}),
        json!({"advances": [one(0, "23", "100.00"), one(0, "23", "100.01")]}),
        json!({"advances": [one(0, "23", "40.00"), one(0, "23", "60.00")], "payments": cash("150.00")}),
        json!({"payments": cash("249.99")}),
        json!({"payments": cash("250.01")}),
        json!({"advances": [one(3, "23", "1.00")]}),
        json!({"advances": [one(0, "8", "1.00")]}),
        json!({"payments": cash("-1.00")}),
        json!({"payments": cash("NaN")}),
        json!({"payments": cash("Infinity")}),
        json!({"payments": cash("0.001")}),
        json!({"payments": cash("100000000.00")}),
        json!({"advances": [one(0, "23", "-1.00")]}),
        json!({"total": "249.99"}),
        json!({"total": "100000000.00"}),
        json!({"lines": [line(&json!({"unitPrice": "100.001", "total": "200.00"})), job_json()["lines"][1].clone()]}),
        json!({"lines": [line(&json!({"vatSlot": 1}))]}),
        json!({"lines": [line(&json!({"name": "Кузов"})), job_json()["lines"][1].clone()]}),
        json!({"companyName": "ООО Ромашка"}),
    ];
    for changes in cases {
        let sim = Simulator::start();
        let (_temporary, dir) = state_dir();
        let mut agent = agent(&sim, &dir, FakeApi::new(vec![job_with(&changes)]));
        poll(&mut agent);
        assert_eq!(agent.api.reports[0].state, Outcome::Failed, "{changes}");
        assert!(!sim.transaction_open());
        assert!(
            !sim.frames()
                .iter()
                .any(|frame| frame.windows(6).any(|window| window == b"trinit")),
            "{changes}"
        );
        let operation = receipt_operation(&dir);
        assert_eq!(
            (
                operation.device,
                operation.settled.map(|settled| settled.state)
            ),
            (Device::NotStarted, Some(JobState::Failed))
        );
    }
}

#[test]
fn failure_reasons_name_the_problem() {
    for (changes, reason) in [
        (
            json!({"advances": [{"vatSlot": 0, "vatRate": "23", "amount": "40.00"}, {"vatSlot": 0, "vatRate": "23", "amount": "60.00"}], "payments": [{"form": "card", "amount": "150.00"}]}),
            "Powtórzony slot VAT zaliczki",
        ),
        (
            json!({"lines": [line(&json!({"unitPrice": "100.001"})), job_json()["lines"][1].clone()]}),
            "Cena",
        ),
        (
            json!({"lines": [line(&json!({"total": "199.99"})), line(&json!({"name": "x", "quantity": "1", "unitPrice": "50.00", "total": "50.01", "vatRate": "8", "vatSlot": 1}))]}),
            "Wartość pozycji",
        ),
        (
            json!({"lines": [line(&json!({"name": "Wymiana 🔧 oleju"})), job_json()["lines"][1].clone()]}),
            "nie drukuje",
        ),
        (json!({"companyName": "x".repeat(41)}), "Nazwa firmy"),
    ] {
        let sim = Simulator::start();
        let (_temporary, dir) = state_dir();
        let mut agent = agent(&sim, &dir, FakeApi::new(vec![job_with(&changes)]));
        poll(&mut agent);
        let result = agent.api.reports[0].result.clone().unwrap_or_default();
        assert!(result.contains(reason), "{result}");
    }
}

#[test]
fn lines_print_as_received_with_half_up_totals() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let changed = job_with(&json!({
        "lines": [
            line(&json!({"quantity": "1.5", "unitPrice": "0.33", "total": "0.50"})),
            line(&json!({
                "quantity": "1", "unitPrice": "0.50", "total": "0.50", "vatRate": "8", "vatSlot": 1
            }))
        ],
        "total": "1.00",
        "payments": [{"form": "card", "amount": "1.00"}]
    }));
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![changed]));
    poll(&mut agent);
    assert_eq!(agent.api.reports[0].state, Outcome::Done);
    let lines: Vec<_> = sim.receipts()[0]
        .lines
        .iter()
        .map(|line| (line["il"].clone(), line["pr"].clone(), line["wa"].clone()))
        .collect();
    assert_eq!(
        lines,
        [
            ("1.5".into(), "33".into(), "50".into()),
            ("1".into(), "50".into(), "50".into())
        ]
    );
}

#[test]
fn a_history_failure_never_blocks_settlement() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    fs::create_dir(dir.path().join("history.jsonl")).unwrap();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    let step = agent.step().unwrap();
    assert!(
        step.notices
            .iter()
            .any(|notice| notice.contains("historii")),
        "{:?}",
        step.notices
    );
    assert_eq!(agent.api.reports.len(), 1);
    assert_eq!(agent.blocked().unwrap(), None);
    let mut agent = restart(&sim, &dir, agent);
    poll(&mut agent);
    assert_eq!(agent.api.reports.len(), 1);
    assert_eq!(sim.receipts().len(), 1);
}

#[test]
fn reports_print_and_leave_nothing_unresolved() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![]));
    agent.run_report(Report::Daily).unwrap();
    agent
        .run_report(Report::Periodic {
            from: date(2026, 8, 1),
            to: date(2026, 8, 31),
            summary: false,
        })
        .unwrap();
    assert_eq!(sim.reports().len(), 2);
    assert_eq!(agent.blocked().unwrap(), None);
}

#[test]
fn an_interrupted_report_blocks_until_confirmed_on_the_same_ready_device() {
    let sim = Simulator::start();
    sim.fault("dailyrep", Fault::Error);
    let (_temporary, dir) = state_dir();
    let mut agent = agent(&sim, &dir, FakeApi::new(vec![job()]));
    assert!(matches!(
        agent.run_report(Report::Daily),
        Err(Error::Printer(posnet::Error::Uncertain(_)))
    ));
    assert_eq!(agent.blocked().unwrap(), Some(Blocked::Report));
    let step = agent.step().unwrap();
    assert_eq!(
        (step.delay, agent.api.jobs.len()),
        (super::RETRY, 1),
        "no receipt while unresolved"
    );
    assert!(matches!(
        agent.run_report(Report::Daily),
        Err(Error::Blocked(_))
    ));

    sim.set_transaction_open(true);
    assert!(agent.resolve_report().is_err(), "the device must be ready");
    sim.set_transaction_open(false);
    agent.resolve_report().unwrap();
    assert_eq!(agent.blocked().unwrap(), None);
    let Some(Operation::Report(ReportOperation {
        acknowledged_at, ..
    })) = Journal::new(&dir).load().unwrap()
    else {
        panic!("the report stays journaled");
    };
    assert!(acknowledged_at.is_some_and(|at| at <= Timestamp::now()));
    assert!(agent.resolve_report().is_err(), "nothing left to resolve");
    assert_eq!(sim.reports().len(), 0);
}
