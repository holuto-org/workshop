use std::{
    fmt,
    sync::{Arc, Mutex, PoisonError},
};

use fiscal::{
    driver::Driver,
    error::Error,
    money::Grosze,
    receipt::{Line, Payment, PaymentForm, Quantity, Receipt, VatSlot},
    report::Report,
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    subscriber,
};
use tracing_subscriber::{
    Layer,
    layer::{Context, SubscriberExt},
    registry,
};

use super::{Log, printer, supported};
use crate::{
    profile,
    simulator::{Fault, Simulator},
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

    fn on_record(&self, _id: &Id, values: &Record<'_>, _context: Context<'_, S>) {
        values.record(&mut self.clone());
    }
}

fn capture(run: impl FnOnce()) -> String {
    let captured = Arc::new(Mutex::new(String::new()));
    subscriber::with_default(registry().with(Capture(Arc::clone(&captured))), run);
    let captured = captured.lock().unwrap_or_else(PoisonError::into_inner);
    captured.clone()
}

#[test]
fn refused_step_traces_command_and_code_but_never_fiscal_payloads() {
    let captured = capture(|| {
        let sim = Simulator::start();
        supported(&sim);
        sim.fault("$b", Fault::Refuse(1234));
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let caps = profile::lookup("NOVITUS HD II ONLINE", "1.40").unwrap();
        let receipt = Receipt::new(
            vec![
                Line::new(
                    "CUSTOMER SECRET NAME",
                    Quantity::parse("1").unwrap(),
                    Grosze::new(93_764_321).unwrap(),
                    VatSlot::new(0).unwrap(),
                    &caps,
                )
                .unwrap(),
            ],
            vec![],
            vec![Payment {
                form: PaymentForm::Card,
                amount: Grosze::new(93_764_321).unwrap(),
            }],
            Some("9876543210".to_owned()),
            "Warsztat",
            &caps,
        )
        .unwrap();
        assert!(matches!(
            driver.print_receipt(&receipt, &expected, &mut Log::default()),
            Err(Error::Uncertain(_))
        ));
    });
    for expected in [
        "device.receipt",
        "timeout_ms=",
        "device_error_code=1234",
        "Printer refused command",
        "Printer operation has an uncertain outcome",
    ] {
        assert!(
            captured.contains(expected),
            "missing {expected}: {captured}"
        );
    }
    assert!(!captured.contains("Receipt printed"), "{captured}");
    for sensitive in [
        "CUSTOMER SECRET NAME",
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
fn operator_wait_and_completed_report_are_traced() {
    let captured = capture(|| {
        let sim = Simulator::start();
        supported(&sim);
        sim.fault("#r", Fault::WaitPolls(1));
        printer(&sim)
            .print_report(Report::Daily, "ABC1234567890", &mut Log::default())
            .unwrap();
    });
    for expected in [
        "device.report",
        "command=\"#r\"",
        "Printer needs operator intervention",
        "Printer resumed after operator intervention",
        "Report printed",
    ] {
        assert!(
            captured.contains(expected),
            "missing {expected}: {captured}"
        );
    }
    assert!(!captured.contains("uncertain outcome"), "{captured}");
}
