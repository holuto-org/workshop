use std::{
    cell::RefCell,
    collections::BTreeMap,
    fmt, fs,
    io::{self, ErrorKind},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

use fiscal::{
    device::Vendor,
    error::Error as PrinterError,
    link::{Candidate, Link, Port},
};
use posnet::simulator::{Fault as PrinterFault, SimReport, Simulator, UNIQUE_NUMBER};
use serde_json::{Value, json};
use tracing::{
    Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id},
    subscriber,
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

use super::{Command, Event, Worker};
use crate::{
    journal::{Device, JobResult, Journal, Operation, ReceiptOperation},
    settings::Settings,
    store::StateDir,
    testing::{ATTEMPT, Fault, Holuto, JOB_ID, PRINTER_ID, job_json, state_dir, with},
};

const WAIT: Duration = Duration::from_secs(5);

mod vendors;

thread_local! {
    static CLOCK: RefCell<Option<Arc<Mutex<Instant>>>> = const { RefCell::new(None) };
}

/// A fixed clock prevents background polls from interleaving navigation assertions.
pub(super) struct ClockScope(Option<Arc<Mutex<Instant>>>);

impl ClockScope {
    pub(super) fn enter(clock: Option<Arc<Mutex<Instant>>>) -> Self {
        Self(CLOCK.replace(clock))
    }
}

impl Drop for ClockScope {
    fn drop(&mut self) {
        CLOCK.set(self.0.take());
    }
}

pub(super) fn clock() -> Option<Arc<Mutex<Instant>>> {
    CLOCK.with_borrow(Clone::clone)
}

pub(super) fn now() -> Instant {
    clock().map_or_else(Instant::now, |clock| *clock.lock().unwrap())
}

#[test]
fn event_debug_omits_operator_text_and_receipt_payloads() {
    let events = [
        Event::Problem(Some(
            "private-customer private-line 98765.43 9876543".to_owned(),
        )),
        Event::Problem(None),
        Event::PortDetected {
            port: "private-port".to_owned(),
            vendor: Vendor::Posnet,
        },
        Event::Connected {
            printer_id: "printer-id".to_owned(),
            unique_number: "device-unique_number".to_owned(),
            vendor: Vendor::Posnet,
            port: "private-port".to_owned(),
        },
        Event::Paired {
            printer_id: "printer-id".to_owned(),
        },
        Event::PairingFailed {
            message: "private-customer private-line 98765.43 9876543".to_owned(),
            failure: super::PairingFailure::Communication,
        },
    ];
    let debug = events.map(|event| format!("{event:?}"));
    assert_eq!(
        debug,
        [
            "Problem(..)",
            "Problem(None)",
            "PortDetected(..)",
            "Connected { printer_id: \"printer-id\", unique_number: \"device-unique_number\", .. }",
            "Paired { printer_id: \"printer-id\" }",
            "PairingFailed { failure: Communication, .. }",
        ]
    );
    for secret in [
        "private-customer",
        "private-line",
        "private-port",
        "98765.43",
        "9876543",
    ] {
        assert!(
            debug.iter().all(|event| !event.contains(secret)),
            "event Debug exposed {secret}"
        );
    }
}

#[test]
fn device_debug_reports_readiness_without_the_printer_company_header() {
    let sim = Simulator::start();
    sim.set_header("private-company");
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let received = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    let _stopped = stop(worker, &events);
    let device = received.last().unwrap();
    assert_eq!(
        format!("{device:?}"),
        "Device { ready: Some(true), blocked: false, report_unresolved: false, \
         orphaned_receipt: false }"
    );
    assert!(!format!("{received:?}").contains("private-company"));
}

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<(tracing::Level, String)>>>);

struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        use std::fmt::Write as _;
        write!(&mut self.0, "{}={value:?};", field.name()).unwrap();
    }
}

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        self.0
            .lock()
            .unwrap()
            .push((*event.metadata().level(), fields.0));
    }
}

#[derive(Debug)]
struct PanicLink;

impl Link for PanicLink {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "the link deliberately panics to prove worker panic containment and redaction"
    )]
    fn open(&self, _port: &str, _baud_rate: u32) -> Result<Box<dyn Port>, PrinterError> {
        panic!("private-token-customer-nip");
    }

    fn candidates(&self) -> Vec<Candidate> {
        Vec::new()
    }
}

#[derive(Debug)]
struct PanicFiscalLink(Arc<dyn Link>);

struct PanicFiscalPort(Box<dyn Port>);

impl Link for PanicFiscalLink {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, PrinterError> {
        Ok(Box::new(PanicFiscalPort(self.0.open(port, baud_rate)?)))
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.0.candidates()
    }

    fn time_scale(&self) -> u32 {
        self.0.time_scale()
    }
}

impl Port for PanicFiscalPort {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "exercise an unwinding fiscal operation"
    )]
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        assert!(
            !bytes.windows(6).any(|part| part == b"trinit"),
            "fiscal panic"
        );
        self.0.write_all(bytes, timeout)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.0.read(buffer, timeout)
    }
}

#[test]
fn a_panic_at_fiscal_dispatch_publishes_the_journal_block_before_stopping() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let id = holuto.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (send, events) = mpsc::channel();
    sim.set_clock("2026-10-16T10:34:00Z".parse().unwrap());
    let worker = Worker::spawn(
        Arc::clone(&dir),
        settings(&holuto, &sim),
        Arc::new(PanicFiscalLink(sim.link())),
        move |event| send.send(event).unwrap(),
    )
    .unwrap();
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    let Some(Operation::Receipt(operation)) = Journal::new(&dir).load().unwrap() else {
        panic!("dispatch intent must survive panic");
    };
    assert!(matches!(operation.device, Device::Dispatching { .. }));
    assert_eq!(holuto.job(id)["state"], "taken");
    assert_eq!(sim.receipts(), []);
    assert!(
        matches!(
            received.as_slice(),
            [
                ..,
                Event::Device {
                    status: None,
                    blocked: true,
                    report_unresolved: false,
                    ..
                },
                Event::Stopped
            ]
        ),
        "{received:?}"
    );
}

fn receive_until(
    events: &Receiver<Event>,
    mut predicate: impl FnMut(&Event) -> bool,
) -> Vec<Event> {
    let mut received = Vec::new();
    loop {
        let event = events
            .recv_timeout(WAIT)
            .expect("worker event before deadline");
        let done = predicate(&event);
        let stopped = matches!(event, Event::Stopped);
        received.push(event);
        if done {
            return received;
        }
        assert!(
            !stopped,
            "worker stopped before the expected event: {received:?}"
        );
    }
}

#[test]
fn a_panicking_worker_reports_a_redacted_error_then_stopped() {
    let (_temporary, dir) = state_dir();
    let capture = Capture(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let (send, events) = mpsc::channel();
    let worker = subscriber::with_default(subscriber, || {
        Worker::spawn(
            Arc::new(dir),
            Settings {
                port: "panic-device".to_owned(),
                ..Settings::default()
            },
            Arc::new(PanicLink),
            move |event| send.send(event).unwrap(),
        )
        .unwrap()
    });
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert!(matches!(
        received.as_slice(),
        [
            Event::Problem(Some(text)),
            Event::Device { status: None, blocked: false, report_unresolved: false, .. },
            Event::Stopped
        ] if text == "Wątek drukarki został przerwany. Sprawdź wydruk przed \
                        ponownym uruchomieniem."
    ));
    assert_eq!(
        *capture.0.lock().unwrap(),
        [(tracing::Level::ERROR, "message=Worker panicked;".to_owned())]
    );
    assert!(!format!("{received:?}").contains("private-token-customer-nip"));
}
fn settings(holuto: &Holuto, sim: &Simulator) -> Settings {
    Settings {
        api_url: holuto.url().to_owned(),
        token: holuto.token().into(),
        port: sim.address().to_owned(),
        vendor: Some(Vendor::Posnet),
        ..Settings::default()
    }
}

fn spawn(holuto: &Holuto, sim: &Simulator, dir: &Arc<StateDir>) -> (Worker, Receiver<Event>) {
    spawn_with_settings(sim, dir, settings(holuto, sim))
}

fn spawn_with_settings(
    sim: &Simulator,
    dir: &Arc<StateDir>,
    settings: Settings,
) -> (Worker, Receiver<Event>) {
    sim.set_clock("2026-10-16T10:34:00Z".parse().unwrap());
    let (send, events) = mpsc::channel();
    let worker = Worker::spawn(Arc::clone(dir), settings, sim.link(), move |event| {
        send.send(event).unwrap();
    })
    .unwrap();
    (worker, events)
}

fn stop(worker: Worker, events: &Receiver<Event>) -> Vec<Event> {
    worker.stop();
    let received = receive_until(events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert_eq!(
        events.try_recv().unwrap_err(),
        mpsc::TryRecvError::Disconnected
    );
    received
}

fn hold_http(holuto: &Holuto, method: &str, path: &str) -> (Receiver<()>, Sender<()>) {
    let (reached, arrival) = mpsc::channel();
    let (release, released) = mpsc::channel();
    holuto.fault(
        method,
        path,
        Fault::Hold {
            reached,
            release: released,
        },
    );
    (arrival, release)
}

fn problem(status: u16) -> Fault {
    Fault::Problem {
        status,
        code: "unavailable".to_owned(),
        detail: "backend unavailable".to_owned(),
    }
}

/// Wakes a worker waiting to retry. With nothing to resolve the command is refused without
/// printer or Holuto traffic.
fn wake(worker: &Worker) {
    worker.submit(Command::ResolveReport);
}

/// Events through the first poll after the next completed command.
fn receive_poll_after_command(events: &Receiver<Event>) -> Vec<Event> {
    let mut completed = false;
    receive_until(events, |event| {
        completed |= matches!(event, Event::CommandCompleted);
        completed && matches!(event, Event::Device { .. })
    })
}

#[test]
fn a_real_spawn_connects_with_the_bound_printer_and_stops_cooperatively() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let received = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    assert!(received.iter().any(|event| matches!(event,
        Event::Connected { printer_id, unique_number, vendor, port }
        if printer_id == &PRINTER_ID.to_string()
            && unique_number == UNIQUE_NUMBER && *vendor == Vendor::Posnet && port == sim.address()
    )));
    assert!(
        !received
            .iter()
            .any(|event| matches!(event, Event::PortDetected { .. }))
    );
    let _stopped = stop(worker, &events);
}

#[test]
fn startup_transient_http_failures_wait_for_retry_and_can_be_stopped() {
    for status in [403, 408, 429, 500, 502, 503, 599] {
        let sim = Simulator::start();
        let holuto = Holuto::new();
        holuto.fault("POST", "/agent/session", problem(status));
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let _clock = ClockScope::enter(Some(Arc::new(Mutex::new(Instant::now()))));
        let (worker, events) = spawn(&holuto, &sim, &dir);
        let expected = format!(
            "Nie można połączyć z Holuto: Holuto odpowiedziało błędem {status}: \
             backend unavailable. Ponowna próba za 15 sekund."
        );
        let _retrying = receive_until(
            &events,
            |event| matches!(event, Event::Problem(Some(text)) if text == &expected),
        );
        let stopped = stop(worker, &events);
        assert!(matches!(stopped.last(), Some(Event::Stopped)));
        assert!(stopped.iter().any(|event| matches!(
            event,
            Event::Device {
                status: None,
                blocked: false,
                report_unresolved: false,
                ..
            }
        )));
        assert_eq!(
            holuto
                .requests()
                .iter()
                .map(|request| request.path.as_str())
                .collect::<Vec<_>>(),
            ["/agent/session"]
        );
        assert_eq!(sim.receipts(), []);
        assert_eq!(sim.reports(), []);
        assert_eq!(Journal::new(&dir).load().unwrap(), None);
    }
}

#[test]
fn startup_contract_failures_stop_without_retry_or_fiscal_effects() {
    for (fault, expected) in [
        (
            problem(400),
            "Błąd: Holuto odpowiedziało błędem 400: backend unavailable",
        ),
        (
            problem(409),
            "Błąd: Holuto odpowiedziało błędem 409: backend unavailable",
        ),
        (
            problem(422),
            "Błąd: Holuto odpowiedziało błędem 422: backend unavailable",
        ),
        (
            problem(499),
            "Błąd: Holuto odpowiedziało błędem 499: backend unavailable",
        ),
    ] {
        let sim = Simulator::start();
        let holuto = Holuto::new();
        holuto.fault("POST", "/agent/session", fault);
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let (worker, events) = spawn(&holuto, &sim, &dir);
        let received = receive_until(&events, |event| matches!(event, Event::Stopped));
        worker.join();
        assert!(matches!(
            received.as_slice(),
            [
                Event::Problem(Some(text)),
                Event::Device { status: None, blocked: false, report_unresolved: false, .. },
                Event::Stopped
            ] if text == expected
        ));
        assert_eq!(
            holuto
                .requests()
                .iter()
                .map(|request| request.path.as_str())
                .collect::<Vec<_>>(),
            ["/agent/session"]
        );
        assert_eq!(sim.receipts(), []);
        assert_eq!(sim.reports(), []);
        assert_eq!(Journal::new(&dir).load().unwrap(), None);
    }
}

#[test]
fn printer_unavailable_at_start_is_retried_until_it_answers() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    // Another holder keeps the port unopenable, like a printer switched on after login.
    let busy = sim.link().open(sim.address(), 9600).unwrap();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let clock = Arc::new(Mutex::new(Instant::now()));
    let _clock = ClockScope::enter(Some(Arc::clone(&clock)));
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let received = receive_until(&events, |event| {
        matches!(event,
            Event::Problem(Some(text)) if text.contains("Ponowna próba za 15 sekund")
        )
    });
    assert!(matches!(
        received.last(),
        Some(Event::Problem(Some(text)))
            if text.starts_with("Drukarka niedostępna: ")
    ));
    assert!(
        holuto.requests().is_empty(),
        "no Holuto session without a printer"
    );
    drop(busy);
    *clock.lock().unwrap() += Duration::from_secs(15);
    wake(&worker);
    let received = receive_poll_after_command(&events);
    assert!(received.iter().any(
        |event| matches!(event, Event::Connected { unique_number, .. }
                if unique_number == UNIQUE_NUMBER)
    ));
    assert!(
        received
            .windows(2)
            .any(|pair| matches!(pair, [Event::Connected { .. }, Event::Problem(None)])),
        "a successful connection clears the unavailable-printer problem: {received:?}"
    );
    let _stopped = stop(worker, &events);
}

/// Discovery that spends the whole retry interval, like several silent ports.
#[derive(Debug)]
struct SlowAbsentLink {
    clock: Arc<Mutex<Instant>>,
    opened: Mutex<usize>,
}

impl Link for SlowAbsentLink {
    #[expect(
        clippy::unwrap_in_result,
        reason = "a poisoned test lock is a test failure, not a link error"
    )]
    fn open(&self, port: &str, _baud_rate: u32) -> Result<Box<dyn Port>, PrinterError> {
        *self.opened.lock().unwrap() += 1;
        *self.clock.lock().unwrap() += Duration::from_secs(20);
        Err(PrinterError::Open {
            port: port.to_owned(),
            source: ErrorKind::NotFound.into(),
        })
    }

    fn candidates(&self) -> Vec<Candidate> {
        Vec::new()
    }
}

#[test]
fn slow_startup_detection_still_waits_the_full_retry_interval() {
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let clock = Arc::new(Mutex::new(Instant::now()));
    let _clock = ClockScope::enter(Some(Arc::clone(&clock)));
    let link = Arc::new(SlowAbsentLink {
        clock: Arc::clone(&clock),
        opened: Mutex::new(0),
    });
    let (sender, events) = mpsc::channel();
    let worker = Worker::spawn(
        Arc::new(dir),
        Settings {
            api_url: holuto.url().to_owned(),
            token: holuto.token().into(),
            port: "silent".to_owned(),
            vendor: Some(Vendor::Posnet),
            ..Settings::default()
        },
        Arc::<SlowAbsentLink>::clone(&link),
        move |event| sender.send(event).unwrap(),
    )
    .unwrap();
    let _retrying = receive_until(&events, |event| {
        matches!(event,
            Event::Problem(Some(text)) if text.contains("Ponowna próba za 15 sekund")
        )
    });
    // Wakes the wait without moving the clock: the retry is still 15 s away.
    wake(&worker);
    let stopped = stop(worker, &events);
    assert!(
        matches!(
            stopped.as_slice(),
            [
                Event::Device {
                    status: None,
                    blocked: false,
                    report_unresolved: false,
                    ..
                },
                Event::Stopped
            ]
        ),
        "{stopped:?}"
    );
    assert_eq!(*link.opened.lock().unwrap(), 1);
}

#[test]
fn a_lost_startup_reply_is_retryable_without_touching_the_printer_again() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let id = holuto.enqueue(job_json());
    holuto.fault("POST", "/agent/session", Fault::DropAfterCommit);
    let (arrival, release) = hold_http(&holuto, "POST", "/agent/session");
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let clock = Arc::new(Mutex::new(Instant::now()));
    let _clock = ClockScope::enter(Some(Arc::clone(&clock)));
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let _received = receive_until(&events, |event| {
        matches!(event,
            Event::Problem(Some(text)) if text.contains("Ponowna próba za 15 sekund")
        )
    });
    let startup_frames = sim.frames();
    assert_eq!(holuto.job(id)["state"], "queued");
    assert_eq!(fiscal_frames(&sim), Vec::<Vec<u8>>::new());
    *clock.lock().unwrap() += Duration::from_secs(15);
    wake(&worker);
    arrival.recv_timeout(WAIT).unwrap();
    let retry_frames = sim.frames();
    let state_before_session = holuto.job(id)["state"].clone();
    release.send(()).unwrap();
    let received = receive_poll_after_command(&events);
    let _stopped = stop(worker, &events);
    assert_eq!(retry_frames, startup_frames);
    assert_eq!(state_before_session, "queued");
    assert!(
        received
            .iter()
            .any(|event| matches!(event, Event::Connected { .. }))
    );
    assert_eq!(
        holuto
            .requests()
            .iter()
            .filter(|request| request.path == "/agent/session")
            .count(),
        2
    );
    assert_eq!(holuto.job(id)["state"], "done");
    assert_eq!(sim.receipts().len(), 1);
}

#[test]
fn startup_unauthorized_stops_instead_of_retrying() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    holuto.unauthorized(true);
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert!(matches!(
        received.as_slice(),
        [
            Event::Problem(Some(text)),
            Event::Device { status: None, blocked: false, report_unresolved: false, .. },
            Event::Stopped
        ] if text == "Błąd: Holuto odrzuciło parowanie drukarki (401). \
                        Sparuj drukarkę ponownie w Ustawieniach."
    ));
    assert_eq!(
        holuto
            .requests()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        ["/agent/session"]
    );
}

#[test]
fn detecting_a_port_emits_an_event_without_creating_or_rewriting_settings() {
    for existing in [false, true] {
        let sim = Simulator::start();
        let holuto = Holuto::new();
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let mut config = settings(&holuto, &sim);
        config.port.clear();
        let initial = existing.then(|| Settings {
            telemetry_enabled: false,
            ..config.clone()
        });
        let before = initial.as_ref().map(|initial| {
            initial.save(&dir).unwrap();
            fs::read(dir.file("settings.json")).unwrap()
        });
        let (worker, events) = spawn_with_settings(&sim, &dir, config);
        let received = receive_until(&events, |event| matches!(event, Event::Device { .. }));
        let _stopped = stop(worker, &events);
        assert_eq!(
            received
                .iter()
                .filter_map(|event| match event {
                    Event::PortDetected {
                        port,
                        vendor: Vendor::Posnet,
                    } => Some(port.as_str()),
                    Event::Problem(_)
                    | Event::PortDetected { .. }
                    | Event::Connected { .. }
                    | Event::Paired { .. }
                    | Event::PairingFailed { .. }
                    | Event::Device { .. }
                    | Event::CommandCompleted
                    | Event::Stopped => None,
                })
                .collect::<Vec<_>>(),
            [sim.address()]
        );
        assert!(
            received.iter().any(
                |event| matches!(event, Event::Connected { port, .. } if port == sim.address())
            )
        );
        assert_eq!(Settings::load(&dir).unwrap(), initial);
        if let Some(before) = before {
            assert_eq!(fs::read(dir.file("settings.json")).unwrap(), before);
        }
    }
}

#[test]
fn a_queued_buyer_nip_reaches_the_printed_receipt() {
    const NIP: &str = "5261040828";
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let id = holuto.enqueue(with(job_json(), &json!({"buyerNip": NIP})));
    let hold = sim.hold("trinit");
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    assert!(hold.reached(WAIT), "the queued receipt reached the printer");
    let before_release = sim.receipts();
    hold.release();
    assert_eq!(before_release, []);
    let _printed = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    let _stopped = stop(worker, &events);
    let receipts = sim.receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].buyer_nip.as_deref(), Some(NIP));
    let job = holuto.job(id);
    assert_eq!(job["state"], "done");
    assert_eq!(job["buyerNip"], NIP);
    assert_eq!(job["receiptNumber"], "1");
}

#[test]
fn a_refused_command_does_not_delay_the_following_report() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let (arrival, release) = hold_http(&holuto, "POST", "/agent/heartbeat");
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let _clock = ClockScope::enter(Some(Arc::new(Mutex::new(Instant::now()))));
    let (worker, events) = spawn(&holuto, &sim, &dir);
    arrival.recv_timeout(WAIT).unwrap();
    worker.submit(Command::ResolveReport);
    worker.submit(Command::DailyReport);
    release.send(()).unwrap();
    let mut completed = 0;
    let received = receive_until(&events, |event| {
        if matches!(event, Event::CommandCompleted) {
            completed += 1;
        }
        completed == 2
    });
    let _stopped = stop(worker, &events);
    assert!(
        received
            .iter()
            .any(|event| matches!(event, Event::Problem(Some(text))
        if text == "Błąd: Brak przerwanego raportu do wyjaśnienia."))
    );
    assert_eq!(
        sim.reports(),
        [SimReport {
            command: "dailyrep".to_owned(),
            params: BTreeMap::from([("da".to_owned(), "2026-10-16".to_owned())]),
        }]
    );
    assert_eq!(sim.receipts(), []);
}

#[test]
fn an_unsupported_vat_warning_is_shown_once_and_never_hides_a_command_result() {
    let sim = Simulator::start();
    sim.set_vat(["23,00", "8,00", "5,00", "0,00", "100,00", "7,00", "101,00"]);
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let _clock = ClockScope::enter(Some(Arc::new(Mutex::new(Instant::now()))));
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let mut received = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    worker.submit(Command::ResolveReport);
    received.extend(receive_poll_after_command(&events));
    let _stopped = stop(worker, &events);
    let statuses: Vec<_> = received
        .iter()
        .filter_map(|event| match event {
            Event::Problem(Some(text)) => Some(text.as_str()),
            Event::Problem(None)
            | Event::PortDetected { .. }
            | Event::Connected { .. }
            | Event::Paired { .. }
            | Event::PairingFailed { .. }
            | Event::Device { .. }
            | Event::CommandCompleted
            | Event::Stopped => None,
        })
        .collect();
    assert_eq!(
        statuses,
        [
            "Pomijam nieobsługiwaną stawkę VAT w slocie F · 7%",
            "Błąd: Brak przerwanego raportu do wyjaśnienia.",
        ],
        "an unchanged VAT table is not re-announced by each poll"
    );
}

#[test]
fn command_unauthorized_releases_the_submission_guard_then_stops() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    print_with_unconfirmed_result(&sim, &holuto, &dir);
    let (arrival, release) = hold_http(&holuto, "POST", "/agent/session");
    let (worker, events) = spawn(&holuto, &sim, &dir);
    arrival.recv_timeout(WAIT).unwrap();
    // The report first settles the pending receipt, and Holuto refuses that result.
    worker.submit(Command::DailyReport);
    holuto.fault(
        "POST",
        &format!("/agent/jobs/{JOB_ID}/result"),
        problem(401),
    );
    release.send(()).unwrap();
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert!(
        received
            .iter()
            .any(|event| matches!(event, Event::Problem(Some(text))
        if text == "Błąd: Holuto odrzuciło parowanie drukarki (401). \
                    Sparuj drukarkę ponownie w Ustawieniach.")),
        "authorization failure reaches the operator"
    );
    assert!(
        matches!(
            received
                .iter()
                .filter(|event| matches!(event, Event::CommandCompleted | Event::Stopped))
                .collect::<Vec<_>>()
                .as_slice(),
            [Event::CommandCompleted, Event::Stopped]
        ),
        "submission guard is released before shutdown"
    );
    assert_eq!(
        sim.receipts().len(),
        1,
        "the pending receipt is never reprinted"
    );
    assert_eq!(
        sim.reports(),
        [],
        "the refused settlement blocks the report"
    );
}

#[test]
fn poll_unauthorized_after_printing_publishes_the_unsettled_journal_before_stopping() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let id = holuto.enqueue(job_json());
    holuto.fault("POST", &format!("/agent/jobs/{id}/result"), problem(401));
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert_eq!(sim.receipts().len(), 1);
    assert_eq!(holuto.job(id)["state"], "taken");
    let Some(Operation::Receipt(operation)) = Journal::new(&dir).load().unwrap() else {
        panic!("unsettled receipt evidence must survive shutdown");
    };
    assert!(matches!(operation.device, Device::Completed { .. }));
    assert!(
        matches!(
            received
                .iter()
                .rev()
                .find(|event| matches!(event, Event::Device { .. })),
            Some(Event::Device {
                blocked: true,
                report_unresolved: false,
                ..
            })
        ),
        "{received:?}"
    );
}

#[test]
fn stop_finishes_the_current_receipt_and_does_not_start_a_queued_report() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let id = holuto.enqueue(job_json());
    let hold = sim.hold("trend");
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    assert!(hold.reached(WAIT), "receipt reached its final command");
    let Some(Operation::Receipt(operation)) = Journal::new(&dir).load().unwrap() else {
        panic!("receipt journal");
    };
    assert!(matches!(operation.device, Device::Dispatching { .. }));
    worker.submit(Command::DailyReport);
    worker.stop();
    let pending = events.try_iter().collect::<Vec<_>>();
    assert!(!pending.iter().any(|event| matches!(event, Event::Stopped)));
    assert_eq!(sim.receipts(), []);
    assert_eq!(sim.reports(), []);
    hold.release();
    let _received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert_eq!(
        sim.receipts()
            .iter()
            .map(|receipt| receipt.total)
            .collect::<Vec<_>>(),
        [25_000]
    );
    assert_eq!(sim.reports(), []);
    assert_eq!(holuto.job(id)["state"], "done");
    assert_eq!(holuto.job(id)["receiptNumber"], "1");
    assert!(!super::journal_block(&dir).blocked);
}

#[test]
fn a_report_submitted_during_a_receipt_waits_for_the_receipt_to_finish() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let _id = holuto.enqueue(job_json());
    let hold = sim.hold("trend");
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    assert!(hold.reached(WAIT));
    worker.submit(Command::DailyReport);
    assert_eq!(sim.reports(), []);
    hold.release();
    let received = receive_until(&events, |event| matches!(event, Event::CommandCompleted));
    let _stopped = stop(worker, &events);
    let receipt_position = received
        .iter()
        .position(|event| matches!(event, Event::Device { .. }))
        .unwrap();
    let report_position = received
        .iter()
        .rposition(|event| matches!(event, Event::Problem(None)))
        .unwrap();
    assert!(receipt_position < report_position);
    assert_eq!(
        sim.receipts()
            .iter()
            .map(|receipt| receipt.total)
            .collect::<Vec<_>>(),
        [25_000]
    );
    assert_eq!(
        sim.reports()
            .iter()
            .map(|report| report.command.as_str())
            .collect::<Vec<_>>(),
        ["dailyrep"]
    );
}

#[test]
fn holuto_unauthorized_is_not_hidden_by_a_simultaneous_printer_error() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let _first = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    sim.fault("getrealid", PrinterFault::Error);
    holuto.fault("POST", "/agent/heartbeat", problem(401));
    wake(&worker);
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert!(
        received
            .iter()
            .any(|event| matches!(event, Event::Problem(Some(text))
        if text == "Błąd: Holuto odrzuciło parowanie drukarki (401). \
                    Sparuj drukarkę ponownie w Ustawieniach."))
    );
    assert_eq!(sim.receipts(), []);
}

fn print_with_unconfirmed_result(sim: &Simulator, holuto: &Holuto, dir: &Arc<StateDir>) {
    let _id = holuto.enqueue(job_json());
    holuto.fault(
        "POST",
        &format!("/agent/jobs/{JOB_ID}/result"),
        problem(503),
    );
    let (worker, events) = spawn(holuto, sim, dir);
    let _failed = receive_until(&events, |event| {
        matches!(event, Event::Problem(Some(text))
        if text == "Błąd: Holuto odpowiedziało błędem 503: backend unavailable")
    });
    let _stopped = stop(worker, &events);
}

#[test]
fn settlement_reports_the_result_even_when_the_next_printer_probe_fails() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    print_with_unconfirmed_result(&sim, &holuto, &dir);
    let (arrival, release) = hold_http(&holuto, "POST", "/agent/session");
    let (worker, events) = spawn(&holuto, &sim, &dir);
    arrival.recv_timeout(WAIT).unwrap();
    sim.fault("getrealid", PrinterFault::Error);
    release.send(()).unwrap();
    let _failed_probe = receive_until(&events, |event| {
        matches!(event, Event::Problem(Some(text))
        if text == "Błąd: Drukarka zwróciła błąd 2000 (getrealid).")
    });
    let _stopped = stop(worker, &events);
    assert_eq!(holuto.job(JOB_ID)["state"], "done");
    assert_eq!(holuto.job(JOB_ID)["receiptNumber"], "1");
    assert_eq!(
        sim.receipts()
            .iter()
            .map(|receipt| receipt.total)
            .collect::<Vec<_>>(),
        [25_000]
    );
}

#[test]
fn settlement_before_a_report_reports_the_result_without_reprinting() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    print_with_unconfirmed_result(&sim, &holuto, &dir);
    let (arrival, release) = hold_http(&holuto, "POST", "/agent/session");
    let (worker, events) = spawn(&holuto, &sim, &dir);
    arrival.recv_timeout(WAIT).unwrap();
    worker.submit(Command::DailyReport);
    release.send(()).unwrap();
    let _reported = receive_until(&events, |event| matches!(event, Event::CommandCompleted));
    let _stopped = stop(worker, &events);
    assert_eq!(holuto.job(JOB_ID)["state"], "done");
    assert_eq!(holuto.job(JOB_ID)["receiptNumber"], "1");
    assert_eq!(
        sim.receipts()
            .iter()
            .map(|receipt| receipt.total)
            .collect::<Vec<_>>(),
        [25_000]
    );
    assert_eq!(
        sim.reports()
            .iter()
            .map(|report| report.command.as_str())
            .collect::<Vec<_>>(),
        ["dailyrep"]
    );
}

#[test]
fn stop_finishes_the_current_report_before_releasing_the_worker() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let hold = sim.hold("dailyrep");
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let _first = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    worker.submit(Command::DailyReport);
    assert!(hold.reached(WAIT));
    worker.stop();
    let pending = events.try_iter().collect::<Vec<_>>();
    assert!(!pending.iter().any(|event| matches!(event, Event::Stopped)));
    assert_eq!(sim.reports(), []);
    hold.release();
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert!(
        received
            .iter()
            .any(|event| matches!(event, Event::Device { blocked: false, .. })),
        "the completed report does not leave the printer blocked"
    );
    assert!(
        matches!(
            received
                .iter()
                .filter(|event| matches!(event, Event::CommandCompleted | Event::Stopped))
                .collect::<Vec<_>>()
                .as_slice(),
            [Event::CommandCompleted, Event::Stopped]
        ),
        "the report finishes before shutdown"
    );
    assert_eq!(
        sim.reports(),
        [SimReport {
            command: "dailyrep".to_owned(),
            params: BTreeMap::from([("da".to_owned(), "2026-10-16".to_owned())]),
        }]
    );
    let Some(Operation::Report(operation)) = Journal::new(&dir).load().unwrap() else {
        panic!("report journal");
    };
    assert_eq!(
        operation.device,
        Device::Completed {
            receipt_number: None
        }
    );
}

#[test]
fn a_dropped_worker_handle_finishes_the_report_without_claiming_another_job() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let hold = sim.hold("dailyrep");
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let _first = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    worker.submit(Command::DailyReport);
    assert!(hold.reached(WAIT));
    let id = holuto.enqueue(job_json());
    drop(worker);
    hold.release();
    let _received = receive_until(&events, |event| matches!(event, Event::Stopped));
    assert_eq!(sim.reports().len(), 1, "the started report finishes");
    assert_eq!(
        sim.receipts(),
        [],
        "no new fiscal operation after the owner left"
    );
    assert_eq!(holuto.job(id)["state"], "queued");
}

#[test]
fn an_orphaned_print_is_acknowledged_locally_without_reprinting_or_reporting() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    print_with_unconfirmed_result(&sim, &holuto, &dir);
    holuto.set_job(JOB_ID, &json!({"state": "failed"}));
    let reports_before = holuto
        .requests()
        .iter()
        .filter(|request| request.path.ends_with("/result"))
        .count();
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let _blocked = receive_until(&events, |event| {
        matches!(
            event,
            Event::Device {
                blocked: true,
                orphaned_receipt: Some(_),
                ..
            }
        )
    });
    worker.submit(Command::AcknowledgeOrphanedReceipt);
    let received = receive_until(&events, |event| matches!(event, Event::CommandCompleted));
    let _stopped = stop(worker, &events);
    assert!(received.iter().any(|event| matches!(
        event,
        Event::Device {
            blocked: false,
            orphaned_receipt: None,
            ..
        }
    )));
    assert_eq!(holuto.job(JOB_ID)["state"], "failed");
    assert_eq!(
        sim.receipts()
            .iter()
            .map(|receipt| receipt.total)
            .collect::<Vec<_>>(),
        [25_000]
    );
    assert_eq!(
        holuto
            .requests()
            .iter()
            .filter(|request| request.path.ends_with("/result"))
            .count(),
        reports_before
    );
    assert_eq!(sim.reports(), []);
}

#[test]
fn an_orphaned_print_publishes_its_evidence_with_the_block() {
    // The desktop quotes this evidence in the acknowledgement confirmation.
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    print_with_unconfirmed_result(&sim, &holuto, &dir);
    holuto.set_job(JOB_ID, &json!({"state": "failed"}));
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let received = receive_until(&events, |event| {
        matches!(
            event,
            Event::Device {
                blocked: true,
                orphaned_receipt: Some(_),
                ..
            }
        )
    });
    let _stopped = stop(worker, &events);
    let expected = format!(
        "Paragon 1 został wydrukowany dla zlecenia 10 (zadanie {JOB_ID}), ale Holuto oznaczyło \
         zadanie jako Failed. Sprawdź wydruk i użyj „Wyjaśnij paragon”."
    );
    let published = received.iter().rev().find_map(|event| {
        if let Event::Device {
            orphaned_receipt, ..
        } = event
        {
            orphaned_receipt.clone()
        } else {
            None
        }
    });
    assert_eq!(
        published.as_ref(),
        Some(&expected),
        "the block carries its evidence"
    );
    let latest = received.iter().rev().find_map(|event| {
        if let Event::Problem(Some(text)) = event {
            Some(text.clone())
        } else {
            None
        }
    });
    assert_eq!(
        latest,
        Some(expected),
        "a generic notice does not replace the evidence in the status bar"
    );
}

#[derive(Clone)]
struct SpanThreads(Arc<Mutex<Vec<(String, ThreadId)>>>);

impl<S: Subscriber> Layer<S> for SpanThreads {
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _context: Context<'_, S>) {
        let mut fields = Fields(String::new());
        attrs.record(&mut fields);
        self.0
            .lock()
            .unwrap()
            .push((fields.0, thread::current().id()));
    }
}

#[test]
fn http_receipts_reports_and_events_share_the_single_worker_thread() {
    let sim = Simulator::start();
    sim.set_clock("2026-10-16T10:34:00Z".parse().unwrap());
    let holuto = Holuto::new();
    let _id = holuto.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let spans = SpanThreads(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::registry().with(spans.clone());
    let _subscriber = subscriber::set_default(subscriber);
    let _gui_span = tracing::info_span!("gui_control", op = "test.gui");
    let (send, events) = mpsc::channel();
    let (threads, event_threads) = mpsc::channel();
    let config = settings(&holuto, &sim);
    let worker = Worker::spawn(Arc::clone(&dir), config, sim.link(), move |event| {
        threads.send(thread::current().id()).unwrap();
        send.send(event).unwrap();
    })
    .unwrap();
    let _printed = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    worker.submit(Command::DailyReport);
    let _reported = receive_until(&events, |event| matches!(event, Event::CommandCompleted));
    let _stopped = stop(worker, &events);
    let event_threads = event_threads.try_iter().collect::<Vec<_>>();
    let worker_thread = *event_threads.first().unwrap();
    assert_ne!(worker_thread, thread::current().id());
    assert!(
        event_threads.iter().all(|thread| *thread == worker_thread),
        "all events originate on the worker thread"
    );
    let spans = spans.0.lock().unwrap();
    assert!(
        spans
            .iter()
            .any(|(fields, id)| fields.contains("test.gui") && *id == thread::current().id()),
        "the capture also observes spans on the GUI thread"
    );
    for operation in ["http.client", "fiscal.receipt", "fiscal.report"] {
        let matching = spans
            .iter()
            .filter(|(fields, _)| fields.contains(operation))
            .collect::<Vec<_>>();
        assert!(
            !matching.is_empty(),
            "the real {operation} operation emits a span"
        );
        assert!(
            matching.iter().all(|(_, thread)| *thread == worker_thread),
            "{operation} must not run on the GUI thread"
        );
    }
    assert_eq!(
        sim.receipts()
            .iter()
            .map(|receipt| receipt.total)
            .collect::<Vec<_>>(),
        [25_000]
    );
    assert_eq!(
        sim.reports()
            .iter()
            .map(|report| report.command.as_str())
            .collect::<Vec<_>>(),
        ["dailyrep"]
    );
}

#[test]
fn a_corrupt_journal_stops_startup_before_any_printer_or_holuto_traffic() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let (_temporary, dir) = state_dir();
    fs::write(dir.file("operation.json"), b"broken").unwrap();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert!(
        received
            .iter()
            .any(|event| matches!(event, Event::Problem(Some(text))
        if text == "Błąd: Uszkodzony rejestr operacji lub nieobsługiwany format. \
                    Drukowanie zablokowane; nie usuwaj pliku."))
    );
    assert!(
        !received
            .iter()
            .any(|event| matches!(event, Event::Connected { .. } | Event::PortDetected { .. }))
    );
    assert_eq!(sim.frames(), Vec::<Vec<u8>>::new(), "no port is probed");
    assert!(holuto.requests().is_empty(), "no session is opened");
    assert_eq!(fs::read(dir.file("operation.json")).unwrap(), b"broken");
}

const FISCAL_COMMANDS: [&str; 8] = [
    "trinit",
    "trnipset",
    "trline",
    "tradvance",
    "trpayment",
    "trend",
    "trftrln",
    "trftrend",
];

fn full_receipt_job() -> Value {
    with(
        job_json(),
        &json!({
            "buyerNip": "5261040828",
            "advances": [{"vatLetter": "A", "vatRate": "23", "amount": "100.00"}],
            "payments": [{"form": "card", "amount": "150.00"}]
        }),
    )
}

fn sent_command(frame: &[u8], command: &str) -> bool {
    frame
        .strip_prefix(&[0x02])
        .and_then(|payload| payload.strip_prefix(command.as_bytes()))
        .is_some_and(|remaining| remaining.first() == Some(&b'\t'))
}

fn fiscal_frames(sim: &Simulator) -> Vec<Vec<u8>> {
    sim.frames()
        .into_iter()
        .filter(|frame| {
            FISCAL_COMMANDS
                .iter()
                .any(|command| sent_command(frame, command))
        })
        .collect()
}

#[test]
fn every_fiscal_command_is_durable_before_the_worker_sends_it_and_stop_finishes_the_receipt() {
    for command in FISCAL_COMMANDS {
        let sim = Simulator::start();
        sim.set_header("Inna firma");
        let hold = sim.hold(command);
        let holuto = Holuto::new();
        let id = holuto.enqueue(full_receipt_job());
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let (worker, events) = spawn(&holuto, &sim, &dir);
        assert!(hold.reached(WAIT), "worker must reach held {command}");
        let Some(Operation::Receipt(operation)) = Journal::new(&dir).load().unwrap() else {
            panic!("worker must journal the receipt before {command}");
        };
        assert_eq!(operation.job_id, id, "the claimed job owns {command}");
        assert_eq!(json!(operation.attempt), holuto.job(id)["attempt"]);
        assert_eq!(
            operation.device,
            Device::Dispatching {
                stage: command.to_owned()
            },
            "intent must be durable before {command} reaches the device"
        );
        assert_eq!(operation.result, None);
        assert_eq!(operation.settled, None);
        worker.stop();
        let pending = events.try_iter().collect::<Vec<_>>();
        assert!(
            !pending.iter().any(|event| matches!(event, Event::Stopped)),
            "stop cannot abandon the held {command}"
        );
        hold.release();
        let _received = receive_until(&events, |event| matches!(event, Event::Stopped));
        worker.join();
        assert!(
            !super::journal_block(&dir).blocked,
            "the complete operation settles before shutdown at {command}"
        );
        assert_eq!(holuto.job(id)["state"], "done");
        assert_eq!(holuto.job(id)["receiptNumber"], "1");
        assert_eq!(
            sim.receipts()
                .iter()
                .map(|receipt| receipt.total)
                .collect::<Vec<_>>(),
            [15_000],
            "advances and payments must finish exactly once after stopping at {command}"
        );
        let frames = fiscal_frames(&sim);
        let commands = frames
            .iter()
            .filter_map(|frame| {
                FISCAL_COMMANDS
                    .into_iter()
                    .find(|command| sent_command(frame, command))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            commands,
            [
                "trinit",
                "trnipset",
                "trline",
                "trline",
                "tradvance",
                "trpayment",
                "trend",
                "trftrln",
                "trftrend"
            ],
            "cooperative shutdown preserves the complete command sequence at {command}"
        );
        assert_eq!(sim.reports(), []);
    }
}

#[test]
fn losing_each_fiscal_reply_through_trend_keeps_the_worker_blocked_across_restart_without_replay() {
    // The footer after `trend` cannot make the fiscal record uncertain; see the next test.
    for command in FISCAL_COMMANDS
        .into_iter()
        .take_while(|command| *command != "trftrln")
    {
        let sim = Simulator::start();
        sim.set_header("Inna firma");
        sim.fault(command, PrinterFault::LoseReply);
        let holuto = Holuto::new();
        let id = holuto.enqueue(full_receipt_job());
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let (worker, events) = spawn(&holuto, &sim, &dir);
        let _blocked = receive_until(&events, |event| {
            matches!(event, Event::Device { blocked: true, .. })
        });
        let _stopped = stop(worker, &events);
        assert_eq!(
            holuto.job(id)["state"],
            "unknown",
            "{command} lost its reply"
        );
        let Some(Operation::Receipt(operation)) = Journal::new(&dir).load().unwrap() else {
            panic!("uncertainty at {command} must remain journaled");
        };
        assert_eq!(
            operation.device,
            Device::Dispatching {
                stage: command.to_owned()
            }
        );
        assert!(matches!(operation.result, Some(JobResult::Unknown { .. })));
        assert_eq!(operation.settled, None);
        let before = fiscal_frames(&sim);
        assert_eq!(
            before
                .iter()
                .filter(|frame| sent_command(frame, command))
                .count(),
            1,
            "{command} must never be retried after its effect"
        );
        let (worker, events) = spawn(&holuto, &sim, &dir);
        let _blocked = receive_until(&events, |event| {
            matches!(event, Event::Device { blocked: true, .. })
        });
        let _stopped = stop(worker, &events);
        assert_eq!(
            fiscal_frames(&sim),
            before,
            "restart must not replay any fiscal frame after losing {command}"
        );
        assert_eq!(holuto.job(id)["state"], "unknown");
        assert_eq!(
            Journal::new(&dir).load().unwrap(),
            Some(Operation::Receipt(operation)),
            "restart must preserve the same uncertain attempt and result at {command}"
        );
        assert_eq!(
            holuto
                .requests()
                .iter()
                .filter(|request| request.path == "/agent/jobs/take")
                .count(),
            1,
            "a blocked worker never claims another job after {command}"
        );
        assert_eq!(sim.reports(), []);
    }
}

#[test]
fn losing_a_footer_reply_after_trend_settles_the_printed_receipt_once() {
    for command in ["trftrln", "trftrend"] {
        let sim = Simulator::start();
        sim.set_header("Inna firma");
        sim.fault(command, PrinterFault::LoseReply);
        let holuto = Holuto::new();
        let id = holuto.enqueue(full_receipt_job());
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let (worker, events) = spawn(&holuto, &sim, &dir);
        let _settled = receive_until(&events, |event| {
            matches!(event, Event::Device { .. }) && holuto.job(id)["state"] == "done"
        });
        let _stopped = stop(worker, &events);
        assert_eq!(holuto.job(id)["state"], "done", "{command}");
        assert_eq!(holuto.job(id)["receiptNumber"], "1", "{command}");
        assert_eq!(sim.receipts().len(), 1, "{command}");
        assert_eq!(
            fiscal_frames(&sim)
                .iter()
                .filter(|frame| sent_command(frame, command))
                .count(),
            1,
            "{command} is never resent"
        );
    }
}

fn recovery_cases() -> [(Device, &'static str, bool, JobResult, Value); 4] {
    [
        (
            Device::NotStarted,
            "failed",
            false,
            JobResult::Failed {
                reason: "Przerwano przed wysłaniem pierwszego polecenia fiskalnego.".to_owned(),
            },
            json!({
                "attempt": ATTEMPT,
                "state": "failed",
                "receiptNumber": null,
                "issuedAt": null,
                "result": "Przerwano przed wysłaniem pierwszego polecenia fiskalnego."
            }),
        ),
        (
            Device::Dispatching {
                stage: "trend".to_owned(),
            },
            "unknown",
            true,
            JobResult::Unknown {
                reason: "Przerwano w trakcie drukowania. Sprawdź drukarkę.".to_owned(),
            },
            json!({
                "attempt": ATTEMPT,
                "state": "unknown",
                "receiptNumber": null,
                "issuedAt": null,
                "result": "Przerwano w trakcie drukowania. Sprawdź drukarkę."
            }),
        ),
        (
            Device::Completed {
                receipt_number: Some("42".to_owned()),
            },
            "done",
            false,
            JobResult::Done {
                receipt_number: "42".to_owned(),
            },
            json!({
                "attempt": ATTEMPT,
                "state": "done",
                "receiptNumber": "42",
                "issuedAt": "2025-01-02T12:00:00Z",
                "result": null
            }),
        ),
        (
            Device::Completed {
                receipt_number: None,
            },
            "unknown",
            true,
            JobResult::Unknown {
                reason: "Wydruk zakończony, ale nie odczytano numeru paragonu. Sprawdź wydruk i \
                         rozstrzygnij zadanie w Holuto."
                    .to_owned(),
            },
            json!({
                "attempt": ATTEMPT,
                "state": "unknown",
                "receiptNumber": null,
                "issuedAt": null,
                "result": "Wydruk zakończony, ale nie odczytano numeru paragonu. Sprawdź wydruk i \
                           rozstrzygnij zadanie w Holuto."
            }),
        ),
    ]
}

#[test]
fn worker_recovery_uses_dispatch_evidence_without_ever_reissuing_a_fiscal_command() {
    for (device, remote_state, blocked, result, result_body) in recovery_cases() {
        let sim = Simulator::start();
        let holuto = Holuto::new();
        holuto.insert(job_json());
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        Journal::new(&dir)
            .save(&Operation::Receipt(ReceiptOperation {
                job_id: JOB_ID,
                attempt: ATTEMPT,
                issued_at: "2025-01-02T12:00:00Z".parse().unwrap(),
                vendor: Vendor::Posnet,
                device_unique_number: UNIQUE_NUMBER.to_owned(),
                device: device.clone(),
                summary: None,
                result: None,
                settled: None,
            }))
            .unwrap();
        let (worker, events) = spawn(&holuto, &sim, &dir);
        let _state = receive_until(
            &events,
            |event| matches!(event, Event::Device { blocked: actual, .. } if *actual == blocked),
        );
        let _stopped = stop(worker, &events);
        assert_eq!(holuto.job(JOB_ID)["state"], remote_state, "{device:?}");
        let saved = Journal::new(&dir).load().unwrap();
        let Some(Operation::Receipt(operation)) = &saved else {
            panic!("recovery must retain its evidence for {device:?}");
        };
        assert_eq!(operation.device, device);
        assert_eq!(operation.result.as_ref(), Some(&result));
        assert_eq!(operation.settled.is_none(), blocked);
        assert_eq!(
            holuto
                .requests()
                .into_iter()
                .filter(|request| request.path.ends_with("/result"))
                .map(|request| request.body)
                .collect::<Vec<_>>(),
            [result_body],
            "recovery reports the evidence, not a replacement print, for {device:?}"
        );
        assert_eq!(fiscal_frames(&sim), Vec::<Vec<u8>>::new());
        let (worker, events) = spawn(&holuto, &sim, &dir);
        let _state = receive_until(
            &events,
            |event| matches!(event, Event::Device { blocked: actual, .. } if *actual == blocked),
        );
        let _stopped = stop(worker, &events);
        assert_eq!(fiscal_frames(&sim), Vec::<Vec<u8>>::new());
        assert_eq!(Journal::new(&dir).load().unwrap(), saved);
        assert_eq!(holuto.job(JOB_ID)["state"], remote_state);
        assert_eq!(sim.receipts(), []);
        assert_eq!(sim.reports(), []);
    }
}

#[test]
fn pairing_saves_credentials_before_notifying_and_never_opens_a_printer() {
    let holuto = Holuto::new();
    holuto.pairing_code("ABCD1234");
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let settings = Settings {
        api_url: holuto.url().to_owned(),
        start_on_launch: true,
        ..Settings::default()
    };
    fs::write(dir.file("operation.json"), b"unresolved fiscal evidence").unwrap();
    let (send, events) = mpsc::channel();
    let observed = Arc::clone(&dir);
    let worker = Worker::pair(
        Arc::clone(&dir),
        settings,
        "ABCD1234".to_owned(),
        move |event| {
            if matches!(event, Event::Paired { .. }) {
                let stored = Settings::load(&observed).unwrap().unwrap();
                assert_eq!(stored.token.as_str(), format!("{:0>42}A", 1));
                assert!(stored.start_on_launch);
            }
            send.send(event).unwrap();
        },
    )
    .unwrap();
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert!(matches!(
        received.as_slice(),
        [Event::Paired { printer_id }, Event::Stopped] if printer_id == &PRINTER_ID.to_string()
    ));
    assert_eq!(
        holuto
            .requests()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        ["/agent/pair"]
    );
    assert_eq!(
        fs::read(dir.file("operation.json")).unwrap(),
        b"unresolved fiscal evidence"
    );
}

#[test]
fn pairing_failure_leaves_saved_credentials_and_reports_a_typed_failure() {
    for throttled in [false, true] {
        let holuto = Holuto::new();
        holuto.pairing_throttled(throttled);
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let settings = Settings {
            api_url: holuto.url().to_owned(),
            token: "previous-token".into(),
            ..Settings::default()
        };
        settings.save(&dir).unwrap();
        let before = fs::read(dir.file("settings.json")).unwrap();
        let (send, events) = mpsc::channel();
        let worker = Worker::pair(
            Arc::clone(&dir),
            settings,
            "ABCD1234".to_owned(),
            move |event| {
                send.send(event).unwrap();
            },
        )
        .unwrap();
        let received = receive_until(&events, |event| matches!(event, Event::Stopped));
        worker.join();
        let expected = if throttled {
            super::PairingFailure::Throttled
        } else {
            super::PairingFailure::Invalid
        };
        assert!(matches!(
            received.as_slice(),
            [Event::PairingFailed { failure, .. }, Event::Stopped] if *failure == expected
        ));
        assert_eq!(fs::read(dir.file("settings.json")).unwrap(), before);
    }
}

#[test]
fn pairing_never_announces_success_if_the_credential_cannot_be_saved() {
    let holuto = Holuto::new();
    holuto.pairing_code("ABCD1234");
    let (_temporary, dir) = state_dir();
    fs::create_dir(dir.file("settings.json")).unwrap();
    let (send, events) = mpsc::channel();
    let settings = Settings {
        api_url: holuto.url().to_owned(),
        ..Settings::default()
    };
    let worker = Worker::pair(
        Arc::new(dir),
        settings,
        "ABCD1234".to_owned(),
        move |event| {
            send.send(event).unwrap();
        },
    )
    .unwrap();
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    assert!(matches!(
        received.as_slice(),
        [
            Event::PairingFailed {
                failure: super::PairingFailure::Storage,
                ..
            },
            Event::Stopped
        ]
    ));
}

#[test]
fn pairing_reports_connection_authentication_and_contract_failures_without_saving() {
    for (fault, expected) in [
        (
            Fault::Problem {
                status: 401,
                code: "unauthorized".to_owned(),
                detail: "denied".to_owned(),
            },
            super::PairingFailure::Unauthorized,
        ),
        (
            Fault::Problem {
                status: 503,
                code: "unavailable".to_owned(),
                detail: "try later".to_owned(),
            },
            super::PairingFailure::Communication,
        ),
        (
            Fault::Body {
                status: 200,
                body: b"invalid response".to_vec(),
            },
            super::PairingFailure::Communication,
        ),
        (Fault::DropAfterCommit, super::PairingFailure::Communication),
    ] {
        let holuto = Holuto::new();
        holuto.pairing_code("ABCD1234");
        holuto.fault("POST", "/agent/pair", fault);
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let settings = Settings {
            api_url: holuto.url().to_owned(),
            ..Settings::default()
        };
        let (send, events) = mpsc::channel();
        let worker = Worker::pair(
            Arc::clone(&dir),
            settings,
            "ABCD1234".to_owned(),
            move |event| {
                send.send(event).unwrap();
            },
        )
        .unwrap();
        let received = receive_until(&events, |event| matches!(event, Event::Stopped));
        worker.join();
        assert!(matches!(
            received.as_slice(),
            [Event::PairingFailed { failure, .. }, Event::Stopped] if *failure == expected
        ));
        assert!(!dir.file("settings.json").exists());
        assert_eq!(holuto.requests().len(), 1);
    }
}

#[test]
fn pairing_explains_missing_api_configuration_without_a_request() {
    let (_temporary, dir) = state_dir();
    let (send, events) = mpsc::channel();
    let worker = Worker::pair(
        Arc::new(dir),
        Settings {
            api_url: String::new(),
            ..Settings::default()
        },
        "ABCD1234".to_owned(),
        move |event| {
            send.send(event).unwrap();
        },
    )
    .unwrap();
    let received = receive_until(&events, |event| matches!(event, Event::Stopped));
    worker.join();
    let expected =
        "Brak adresu Holuto. Ustaw go poleceniem: workshop config set --api-url <adres>.";
    assert!(matches!(
        received.as_slice(),
        [Event::PairingFailed {
            message, failure: super::PairingFailure::Invalid,
        }, Event::Stopped] if message == expected
    ));
}

#[test]
fn an_executed_report_with_a_lost_or_disconnected_reply_blocks_restart_until_resolution() {
    for fault in [PrinterFault::LoseReply, PrinterFault::Disconnect] {
        let sim = Simulator::start();
        let holuto = Holuto::new();
        let (_temporary, dir) = state_dir();
        let dir = Arc::new(dir);
        let _clock = ClockScope::enter(Some(Arc::new(Mutex::new(Instant::now()))));
        let (worker, events) = spawn(&holuto, &sim, &dir);
        let _started = receive_until(&events, |event| matches!(event, Event::Device { .. }));
        sim.fault("dailyrep", fault);
        worker.submit(Command::DailyReport);
        let _completed = receive_until(&events, |event| matches!(event, Event::CommandCompleted));
        let _stopped = stop(worker, &events);
        let saved = Journal::new(&dir).load().unwrap();
        let Some(Operation::Report(operation)) = &saved else {
            panic!("executed report must retain its uncertain intent");
        };
        assert_eq!(
            operation.device,
            Device::Dispatching {
                stage: "dailyrep".to_owned()
            }
        );
        assert_eq!(operation.acknowledged_at, None);
        assert_eq!(
            sim.reports(),
            [SimReport {
                command: "dailyrep".to_owned(),
                params: BTreeMap::from([("da".to_owned(), "2026-10-16".to_owned())]),
            }]
        );
        let reports = sim
            .frames()
            .into_iter()
            .filter(|frame| sent_command(frame, "dailyrep"))
            .collect::<Vec<_>>();
        assert_eq!(reports.len(), 1);
        let requests_before_restart = holuto.requests().len();
        let id = holuto.enqueue(job_json());

        let (worker, events) = spawn(&holuto, &sim, &dir);
        let _blocked = receive_until(&events, |event| {
            matches!(
                event,
                Event::Device {
                    blocked: true,
                    report_unresolved: true,
                    ..
                }
            )
        });
        worker.submit(Command::DailyReport);
        let refused = receive_until(&events, |event| matches!(event, Event::CommandCompleted));
        assert!(refused.iter().any(|event| matches!(event,
            Event::Problem(Some(text))
                if text == "Błąd: Najpierw wyjaśnij przerwany raport."
        )));
        assert_eq!(Journal::new(&dir).load().unwrap(), saved);
        assert_eq!(holuto.job(id)["state"], "queued");
        assert_eq!(sim.receipts(), []);
        assert!(
            holuto
                .requests()
                .iter()
                .skip(requests_before_restart)
                .all(|request| request.path != "/agent/jobs/take")
        );

        sim.set_paper(false);
        worker.submit(Command::ResolveReport);
        let _refused = receive_until(&events, |event| matches!(event, Event::CommandCompleted));
        assert_eq!(Journal::new(&dir).load().unwrap(), saved);
        sim.set_paper(true);
        let (arrival, release) = hold_http(&holuto, "POST", "/agent/jobs/take");
        worker.submit(Command::ResolveReport);
        arrival.recv_timeout(WAIT).unwrap();
        let Some(Operation::Report(resolved)) = Journal::new(&dir).load().unwrap() else {
            panic!("resolution must be durable before the next claim");
        };
        assert!(resolved.acknowledged_at.is_some());
        assert_eq!(resolved.device, operation.device);
        let after = sim
            .frames()
            .into_iter()
            .filter(|frame| sent_command(frame, "dailyrep"))
            .collect::<Vec<_>>();
        assert_eq!(after, reports, "resolution never reissues a report");
        release.send(()).unwrap();
        let _printed = receive_until(&events, |event| {
            matches!(event, Event::Device { .. }) && holuto.job(id)["state"] == "done"
        });
        let _stopped = stop(worker, &events);
        assert_eq!(holuto.job(id)["receiptNumber"], "1");
        assert_eq!(holuto.job(id)["state"], "done");
        assert_eq!(sim.receipts().len(), 1);
        assert_eq!(sim.reports().len(), 1);
    }
}

#[test]
fn an_uncertain_print_stays_unknown_in_holuto_and_blocks_locally() {
    let sim = Simulator::start();
    sim.fault("trend", PrinterFault::LoseReply);
    let holuto = Holuto::new();
    let _id = holuto.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let _blocked = receive_until(&events, |event| {
        matches!(event, Event::Device { blocked: true, .. })
            && holuto.job(JOB_ID)["state"] == "unknown"
    });
    let _stopped = stop(worker, &events);
    assert_eq!(holuto.job(JOB_ID)["state"], "unknown");
    assert_eq!(sim.receipts().len(), 1);
    assert!(super::journal_block(&dir).blocked);
}

#[test]
fn a_job_another_instance_takes_is_never_printed_locally() {
    let sim = Simulator::start();
    let holuto = Holuto::new();
    let id = holuto.enqueue(job_json());
    let (arrival, release) = hold_http(&holuto, "POST", "/agent/jobs/take");
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    arrival.recv_timeout(WAIT).unwrap();
    // The token is not exclusive to one state directory: another instance claimed the job.
    holuto.set_job(
        id,
        &json!({"state": "taken", "attempt": ATTEMPT, "takenAt": "2026-10-16T10:00:00Z"}),
    );
    release.send(()).unwrap();
    let _polled = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    let _stopped = stop(worker, &events);
    assert_eq!(holuto.job(id)["state"], "taken");
    assert_eq!(holuto.job(id)["attempt"], json!(ATTEMPT));
    assert_eq!(
        sim.receipts(),
        [],
        "this instance never prints the claimed job"
    );
}

#[test]
fn refused_receipt_is_a_failed_server_job_with_the_reported_reason() {
    let sim = Simulator::start();
    sim.fault("trinit", PrinterFault::Error);
    let holuto = Holuto::new();
    let _id = holuto.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let received = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    let _stopped = stop(worker, &events);
    let reason = holuto.job(JOB_ID)["result"].as_str().unwrap().to_owned();
    assert_eq!(holuto.job(JOB_ID)["state"], "failed");
    assert!(received.iter().any(|event| matches!(event,
        Event::Problem(Some(text))
        if text == &format!("Nie wydrukowano paragonu (zlecenie 10): {reason}")
    )));
    assert_eq!(sim.receipts(), []);
    assert!(!super::journal_block(&dir).blocked);
}

#[test]
fn polling_a_printer_that_is_not_ready_never_claims_a_queued_job() {
    let sim = Simulator::start();
    sim.set_paper(false);
    let holuto = Holuto::new();
    let id = holuto.enqueue(job_json());
    let (_temporary, dir) = state_dir();
    let dir = Arc::new(dir);
    let _clock = ClockScope::enter(Some(Arc::new(Mutex::new(Instant::now()))));
    let (worker, events) = spawn(&holuto, &sim, &dir);
    let _first = receive_until(&events, |event| matches!(event, Event::Device { .. }));
    wake(&worker);
    let _second = receive_poll_after_command(&events);
    let _stopped = stop(worker, &events);
    assert_eq!(holuto.job(id)["state"], "queued");
    assert!(
        holuto
            .requests()
            .iter()
            .all(|request| request.path != "/agent/jobs/take")
    );
    assert_eq!(sim.receipts(), []);
    assert_eq!(Journal::new(&dir).load().unwrap(), None);
}
