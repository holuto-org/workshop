use super::{
    ClockScope, Command, Device, Event, Holuto, Journal, Operation, Settings, Worker, history,
    job_json, problem, receive_until, state_dir, stop,
};
use crate::{
    agent::{Agent, RETRY},
    api::Api,
    device,
    store::StateDir,
    testing::printers::{RoutingLink, Simulator, VENDORS},
};
use fiscal::{
    device::Vendor,
    link::{Candidate, Connection},
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Instant,
};

fn spawn(
    sim: &Simulator,
    holuto: &Holuto,
    dir: &Arc<StateDir>,
    remembered: Option<Vendor>,
) -> (Worker, mpsc::Receiver<Event>) {
    let (sender, receiver) = mpsc::channel();
    let worker = Worker::spawn(
        Arc::clone(dir),
        Settings {
            api_url: holuto.url().to_owned(),
            token: holuto.token().into(),
            port: sim.address().to_owned(),
            vendor: remembered,
            ..Settings::default()
        },
        sim.link(),
        move |event| sender.send(event).unwrap(),
    )
    .unwrap();
    (worker, receiver)
}

#[test]
fn each_vendor_prints_a_receipt_end_to_end() {
    for vendor in VENDORS {
        let sim = Simulator::start(vendor);
        sim.prepare_receipt();
        let holuto = Holuto::new();
        let id = holuto.enqueue(job_json());
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let (worker, events) = spawn(&sim, &holuto, &dir, None);
        let received = receive_until(&events, |event| {
            matches!(event, Event::Device { blocked: false, .. })
        });
        let _stopped = stop(worker, &events);
        assert_eq!(sim.receipts(), 1, "{vendor:?}");
        assert_eq!(sim.reports(), 0);
        assert_eq!(holuto.job(id)["state"], "done", "{vendor:?}");
        assert_eq!(holuto.job(id)["receiptNumber"], "1");
        assert_eq!(
            history::load(&dir)
                .unwrap()
                .iter()
                .map(|entry| entry.job_id)
                .collect::<Vec<_>>(),
            [id]
        );
        assert!(
            received.iter().any(|event| matches!(
                event,
                Event::Device { status: Some(status), blocked: false, .. }
                    if status.vendor == vendor
            )),
            "{vendor:?}"
        );
        assert!(received.iter().any(|event| matches!(
            event,
            Event::PortDetected { port, vendor: detected }
                if port == sim.address() && *detected == vendor
        )));
        assert_eq!(
            holuto
                .requests()
                .iter()
                .find(|request| request.path == "/agent/session")
                .unwrap()
                .body["vendor"],
            json!(vendor)
        );
    }
}

#[test]
fn overdue_daily_report_does_not_allow_a_job_claim() {
    use elzab::simulator::Simulator as ElzabSimulator;

    let sim = ElzabSimulator::start();
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let (printer, status) = device::detect(
        sim.link(),
        Connection {
            port: sim.address().to_owned(),
            baud_rate: 9600,
        },
        Some(Vendor::Elzab),
        false,
    )
    .unwrap();
    let api = Api::new(holuto.url(), holuto.token(), &status.unique_number).unwrap();
    let waiting = |_problem: Option<&str>| {};
    let mut agent = Agent::new(printer, api, &dir, &waiting);
    let _notices = agent.describe(status).unwrap();
    let _session = agent.session().unwrap();
    let id = holuto.enqueue(job_json());
    sim.set_status(
        0,
        (1_u128 << 115) | (1_u128 << 102) | (1_u128 << 98) | 4,
        32,
    );

    assert_eq!(agent.step().unwrap(), RETRY);
    let status = agent.status().unwrap();
    assert!(!status.ready());
    assert!(status.daily_report_allowed());
    assert_eq!(holuto.job(id)["state"], "queued");
    assert!(
        !holuto
            .requests()
            .iter()
            .any(|request| request.path == "/agent/jobs/take")
    );
    assert_eq!(sim.receipts(), 0);
    assert_eq!(sim.reports(), 0);
}

#[test]
fn each_vendor_lost_reply_blocks_across_restart_without_replay() {
    for vendor in VENDORS {
        let sim = Simulator::start(vendor);
        sim.prepare_receipt();
        sim.lose_final_reply();
        let holuto = Holuto::new();
        let id = holuto.enqueue(job_json());
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let (worker, events) = spawn(&sim, &holuto, &dir, None);
        let _blocked = receive_until(&events, |event| {
            matches!(event, Event::Device { blocked: true, .. })
        });
        let _stopped = stop(worker, &events);
        assert_eq!(sim.receipts(), 1, "{vendor:?}");
        assert_eq!(holuto.job(id)["state"], "unknown");
        let saved = Journal::new(&dir).load().unwrap();
        let Some(Operation::Receipt(operation)) = &saved else {
            panic!("uncertain receipt must remain journaled");
        };
        assert_eq!(operation.vendor, vendor);
        assert!(matches!(operation.device, Device::Dispatching { .. }));
        assert_eq!(operation.settled, None);
        let before = sim.fiscal_frames();
        assert_ne!(before, Vec::<Vec<u8>>::new());
        // Recovery must use the journal's brand, not stale settings.
        let (worker, events) = spawn(&sim, &holuto, &dir, Some(Vendor::Posnet));
        let _blocked = receive_until(&events, |event| {
            matches!(event, Event::Device { blocked: true, .. })
        });
        worker.submit(Command::DailyReport);
        let _completed = receive_until(&events, |event| matches!(event, Event::CommandCompleted));
        let _stopped = stop(worker, &events);
        assert_eq!(
            sim.fiscal_frames(),
            before,
            "{vendor:?}: restart or report replayed fiscal traffic"
        );
        assert_eq!(sim.receipts(), 1);
        assert_eq!(sim.reports(), 0);
        assert_eq!(Journal::new(&dir).load().unwrap(), saved);
        assert_eq!(holuto.job(id)["state"], "unknown");
        assert_eq!(
            holuto
                .requests()
                .iter()
                .filter(|request| request.path == "/agent/jobs/take")
                .count(),
            1
        );
    }
}

#[test]
fn blocked_worker_never_enumerates_or_switches_to_an_available_printer() {
    for vendor in VENDORS {
        let sim = Simulator::start(vendor);
        sim.prepare_receipt();
        sim.lose_final_reply();
        let holuto = Holuto::new();
        let _id = holuto.enqueue(job_json());
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let (worker, events) = spawn(&sim, &holuto, &dir, None);
        let _blocked = receive_until(&events, |event| {
            matches!(event, Event::Device { blocked: true, .. })
        });
        let _stopped = stop(worker, &events);
        let saved = Journal::new(&dir).load().unwrap();
        let alternate = Simulator::start(Vendor::Elzab);
        let link = RoutingLink::new(vec![(
            Candidate {
                port: alternate.address().to_owned(),
                hint: Some(Vendor::Elzab),
            },
            alternate.link(),
        )]);
        let (sender, events) = mpsc::channel();
        let worker = Worker::spawn(
            Arc::clone(&dir),
            Settings {
                api_url: holuto.url().to_owned(),
                token: holuto.token().into(),
                port: "unavailable".to_owned(),
                vendor: Some(Vendor::Elzab),
                ..Settings::default()
            },
            Arc::<RoutingLink>::clone(&link),
            move |event| sender.send(event).unwrap(),
        )
        .unwrap();
        let _stopped = receive_until(&events, |event| matches!(event, Event::Stopped));
        worker.join();
        assert_eq!(*link.opened.lock().unwrap(), ["unavailable"]);
        assert_eq!(link.enumeration_count(), 0);
        assert_eq!(alternate.frames(), Vec::<Vec<u8>>::new());
        assert_eq!(Journal::new(&dir).load().unwrap(), saved);
    }
}

fn unqualified(sim: &Simulator, unsupported: bool) {
    match sim {
        Simulator::Novitus(peer) => {
            if unsupported {
                peer.set_identity("NOVITUS UNKNOWN", "99.00");
            } else {
                peer.set_identity("NOVITUS HD II ONLINE", "3.50");
            }
        },
        Simulator::Elzab(peer) => peer.set_code_page(if unsupported { "CP852" } else { "CP1250" }),
        Simulator::Posnet(_) => panic!("this fixture targets unqualified native profiles"),
    }
}

const UNQUALIFIED: [(Vendor, &str); 2] = [
    (Vendor::Novitus, "Nieznany model lub firmware Novitus"),
    (Vendor::Elzab, "Ustaw stronę kodową drukarki na CP1250"),
];

#[test]
fn unsupported_model_reports_not_ready_without_rejected_telemetry_or_stop() {
    for (vendor, reason) in UNQUALIFIED {
        let expected_notice = format!("Drukarka niegotowa: {reason}");
        let sim = Simulator::start(vendor);
        unqualified(&sim, true);
        let holuto = Holuto::new();
        let _id = holuto.enqueue(job_json());
        // Let the test inspect an invalid session instead of panicking on its schema.
        holuto.fault("POST", "/agent/session", problem(400));
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let (worker, events) = spawn(&sim, &holuto, &dir, Some(vendor));
        let received = receive_until(&events, |event| {
            matches!(
                event, Event::Status { text, .. } if text == &expected_notice
            )
        });
        assert!(received.iter().any(|event| matches!(
            event, Event::Device { status: Some(status), .. }
                if !status.ready() && status.description() == reason
        )));
        assert_eq!(holuto.requests(), []);
        let stopped = stop(worker, &events);
        assert!(matches!(stopped.last(), Some(Event::Stopped)));
        assert_eq!(sim.receipts(), 0);
        assert_eq!(sim.reports(), 0);
    }
}

#[test]
fn device_becoming_unsupported_mid_session_stops_heartbeats_and_takes_no_jobs() {
    for (vendor, reason) in UNQUALIFIED {
        let sim = Simulator::start(vendor);
        sim.prepare_receipt();
        let holuto = Holuto::new();
        let (_temporary, dir) = state_dir();
        let (printer, status) = device::detect(
            sim.link(),
            Connection {
                port: sim.address().to_owned(),
                baud_rate: 9600,
            },
            Some(vendor),
            false,
        )
        .unwrap();
        let api = Api::new(holuto.url(), holuto.token(), &status.unique_number).unwrap();
        let waiting = |_problem: Option<&str>| {};
        let mut agent = Agent::new(printer, api, &dir, &waiting);
        let _notices = agent.describe(status).unwrap();
        let _session = agent.session().unwrap();
        let id = holuto.enqueue(job_json());
        holuto.fault("POST", "/agent/heartbeat", problem(400));
        unqualified(&sim, true);
        for _ in 0..2 {
            assert_eq!(agent.step().unwrap(), RETRY);
            assert!(!agent.status().unwrap().ready());
            assert_eq!(
                agent.take_news().notices,
                [format!("Drukarka niegotowa: {reason}")]
            );
        }
        assert_eq!(
            holuto
                .requests()
                .iter()
                .map(|request| request.path.as_str())
                .collect::<Vec<_>>(),
            ["/agent/session"]
        );
        assert_eq!(holuto.job(id)["state"], "queued");
        assert_eq!(sim.receipts(), 0);
        assert_eq!(sim.reports(), 0);
    }
}

#[test]
fn startup_reprobes_unavailable_capabilities_and_connects_when_the_profile_is_restored() {
    for vendor in [Vendor::Novitus, Vendor::Elzab] {
        let sim = Simulator::start(vendor);
        sim.prepare_receipt();
        unqualified(&sim, true);
        let holuto = Holuto::new();
        let id = holuto.enqueue(job_json());
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let clock = Arc::new(Mutex::new(Instant::now()));
        let _clock = ClockScope::enter(Some(Arc::clone(&clock)));
        let (worker, events) = spawn(&sim, &holuto, &dir, Some(vendor));
        let _unready = receive_until(&events, |event| {
            matches!(
                event, Event::Status { text, .. } if text.starts_with("Drukarka niegotowa: ")
            )
        });
        assert_eq!(holuto.requests(), []);
        unqualified(&sim, false);
        *clock.lock().unwrap() += RETRY;
        worker.submit(Command::NextPage);
        let _ready = receive_until(&events, |event| {
            matches!(
                event, Event::Device { status: Some(status), .. } if status.ready()
            )
        });
        let _stopped = stop(worker, &events);
        assert_eq!(holuto.job(id)["state"], "done");
        assert_eq!(sim.receipts(), 1);
        assert_eq!(
            holuto
                .requests()
                .iter()
                .filter(|request| request.path == "/agent/session")
                .count(),
            1
        );
    }
}
