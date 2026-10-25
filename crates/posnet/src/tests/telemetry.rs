use std::{
    fmt,
    sync::{Arc, Mutex, PoisonError},
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

use super::{Recorder, printer, simulator};
use fiscal::{
    driver::Driver,
    error::Error,
    receipt::{PaymentForm, Receipt},
    report::Report,
};
use jiff::civil::date;

use crate::{
    capabilities::capabilities,
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

#[test]
fn a_monthly_report_waits_as_long_as_the_periodic_report_covering_that_month() {
    let captured = Arc::new(Mutex::new(String::new()));
    let subscriber = registry().with(Capture(Arc::clone(&captured)));
    subscriber::with_default(subscriber, || {
        let sim = simulator();
        let device = printer(&sim);
        for report in [
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
                .print_report(report, UNIQUE_NUMBER, &mut Recorder::default())
                .unwrap();
        }
    });
    let captured = captured.lock().unwrap();
    let budget = |command: &str| {
        captured
            .split(&format!("command=\"{command}\";timeout_ms="))
            .nth(1)
            .and_then(|rest| rest.split(';').next())
            .map(str::to_owned)
    };
    let periodic = budget("periodicrepbydates");
    assert!(periodic.is_some(), "{captured}");
    assert_eq!(budget("monthlyrep"), periodic);
}
