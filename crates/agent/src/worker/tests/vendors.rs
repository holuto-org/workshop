use super::{
    Command, Device, Event, Holuto, Journal, Operation, Settings, Worker, history, job_json,
    receive_until, state_dir, stop,
};
use crate::{
    store::StateDir,
    testing::printers::{RoutingLink, Simulator, VENDORS},
};
use fiscal::{device::Vendor, link::Candidate};
use serde_json::json;
use std::sync::{Arc, mpsc};

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
        // An incorrect settings cache cannot change a journal-bound brand.
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
