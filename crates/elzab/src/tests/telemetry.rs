use std::{
    fmt,
    sync::{Arc, Mutex, PoisonError},
};

use fiscal::{driver::Driver, error::Error, report::Report};
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

use super::{Log, printer, receipt};
use crate::simulator::{Fault, Simulator};

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
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.fault("LIT", Fault::Device(450));
        assert!(matches!(
            driver.print_receipt(&receipt(0, false), &expected, &mut Log::default()),
            Err(Error::Uncertain(_))
        ));
    });
    for expected in [
        "device.receipt",
        "command=\"LIT\"",
        "timeout_ms=",
        "device_error_code=450",
        "Printer refused command",
        "Printer operation has an uncertain outcome",
    ] {
        assert!(
            captured.contains(expected),
            "missing {expected}: {captured}"
        );
    }
    assert!(!captured.contains("Receipt printed"), "{captured}");
    for sensitive in ["Łożysko", "1234567890", "123.46", "123,46"] {
        assert!(
            !captured.contains(sensitive),
            "fiscal payload leaked: {sensitive}"
        );
    }
}

#[test]
fn accepted_daily_report_is_traced_as_uncertain_not_printed() {
    let captured = capture(|| {
        let sim = Simulator::start();
        assert!(matches!(
            printer(&sim).print_report(Report::Daily, &sim_unique(&sim), &mut Log::default()),
            Err(Error::Uncertain(_))
        ));
    });
    for expected in [
        "device.report",
        "Printer operation has an uncertain outcome",
    ] {
        assert!(
            captured.contains(expected),
            "missing {expected}: {captured}"
        );
    }
    assert!(!captured.contains("Report printed"), "{captured}");
}

fn sim_unique(sim: &Simulator) -> String {
    printer(sim).probe().unwrap().unique_number
}
