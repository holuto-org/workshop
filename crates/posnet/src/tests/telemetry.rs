use std::{
    fmt, io,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id},
    subscriber,
};
use tracing_subscriber::{
    Layer,
    layer::{Context, SubscriberExt},
    registry,
};

use super::{Recorder, all_steps_receipt, printer, simulator};
use fiscal::{
    driver::Driver,
    error::Error,
    link::{Candidate, Connection, Link, Port},
    receipt::{PaymentForm, Receipt},
    report::Report,
};
use jiff::civil::date;

use crate::{
    capabilities::capabilities,
    frame,
    printer::Printer,
    simulator::{Fault, UNIQUE_NUMBER},
    testing::{line, pay},
};

#[derive(Clone)]
struct Capture(Arc<Mutex<String>>);

impl Visit for Capture {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        use fmt::Write as _;
        let mut captured = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        write!(captured, "{field}={value:?};").unwrap();
    }
}

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        event.record(&mut self.clone());
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _context: Context<'_, S>) {
        attrs.record(&mut self.clone());
    }
}

#[test]
fn command_traces_include_timeouts_and_device_codes_but_never_fiscal_payloads() {
    let captured = Arc::new(Mutex::new(String::new()));
    let subscriber = registry().with(Capture(Arc::clone(&captured)));
    subscriber::with_default(subscriber, || {
        let sim = simulator();
        sim.fault("trline", Fault::Error);
        let device = printer(&sim);
        let status = device.probe().unwrap();
        let receipt = Receipt::new(
            vec![line("CUSTOMER_SECRET_NAME", "1", "937643.21", 0)],
            vec![],
            vec![pay(PaymentForm::Card, 93_764_321)],
            Some("9876543210".to_owned()),
            "COMPANY_SECRET_NAME",
            &capabilities(),
        )
        .unwrap();
        assert!(
            matches!(
                device.print_receipt(&receipt, &status, &mut Recorder::default()),
                Err(Error::Uncertain(_))
            ),
            "refused line is uncertain"
        );
    });
    let captured = captured.lock().unwrap();
    for expected in [
        "device.receipt",
        "command=\"trline\"",
        "timeout_ms=100",
        "device_error_code=2000",
        "uncertain outcome",
    ] {
        assert!(
            captured.contains(expected),
            "missing {expected}: {captured}"
        );
    }
    for sensitive in [
        "CUSTOMER_SECRET_NAME",
        "COMPANY_SECRET_NAME",
        "9876543210",
        "93764321",
        "937643.21",
    ] {
        assert!(
            !captured.contains(sensitive),
            "fiscal payload leaked: {sensitive}"
        );
    }
}

#[derive(Debug)]
struct Deadlines {
    inner: Arc<dyn Link>,
    writes: Arc<Mutex<Vec<(String, Duration)>>>,
}

impl Link for Deadlines {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        Ok(Box::new(DeadlinePort {
            inner: self.inner.open(port, baud_rate)?,
            writes: Arc::clone(&self.writes),
        }))
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.inner.candidates()
    }
}

struct DeadlinePort {
    inner: Box<dyn Port>,
    writes: Arc<Mutex<Vec<(String, Duration)>>>,
}

impl Port for DeadlinePort {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        let (command, _) = frame::decode(bytes).map_err(io::Error::other)?;
        self.writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((command, timeout));
        self.inner.write_all(bytes, timeout)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.inner.read(buffer, timeout)
    }
}

#[test]
fn fiscal_writes_use_the_documented_production_deadlines() {
    let sim = simulator();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let device = Printer::new(
        Arc::new(Deadlines {
            inner: sim.link(),
            writes: Arc::clone(&writes),
        }),
        Connection {
            port: sim.address().to_owned(),
            baud_rate: 9600,
        },
    );
    let expected = device.probe().unwrap();
    let mut journal = Recorder::default();
    let _printed = device
        .print_receipt(&all_steps_receipt(), &expected, &mut journal)
        .unwrap();
    for report in [
        Report::Daily,
        Report::Monthly {
            year: 2026,
            month: 6,
        },
        Report::Periodic {
            from: date(2026, 6, 1),
            to: date(2026, 6, 30),
            summary: false,
        },
    ] {
        device
            .print_report(report, UNIQUE_NUMBER, &mut journal)
            .unwrap();
    }
    let fiscal: Vec<_> = writes
        .lock()
        .unwrap()
        .iter()
        .filter(|(command, _)| {
            command.starts_with("tr") || command.ends_with("rep") || command == "periodicrepbydates"
        })
        .cloned()
        .collect();
    assert_eq!(
        fiscal,
        [
            ("trinit".to_owned(), 10),
            ("trnipset".to_owned(), 10),
            ("trline".to_owned(), 10),
            ("tradvance".to_owned(), 10),
            ("trpayment".to_owned(), 10),
            ("trend".to_owned(), 30),
            ("trftrln".to_owned(), 10),
            ("trftrend".to_owned(), 30),
            ("dailyrep".to_owned(), 120),
            ("monthlyrep".to_owned(), 300),
            ("periodicrepbydates".to_owned(), 300),
        ]
        .map(|(command, seconds)| (command, Duration::from_secs(seconds)))
    );
}
