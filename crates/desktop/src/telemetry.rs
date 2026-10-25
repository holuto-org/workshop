//! Desktop telemetry. Free-form operator, server and panic text is scrubbed locally.
#[path = "telemetry/privacy.rs"]
mod privacy;
#[path = "telemetry/transport.rs"]
mod transport;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use agent::settings::Settings;
use sentry::{
    ClientInitGuard, ClientOptions, Envelope, Transport, TransportFactory, TransportOptions,
    integrations::{
        minidump::MinidumpIntegration,
        tracing::{self as sentry_tracing, EventFilter, SentryLayer},
    },
    protocol::{Breadcrumb, Context, EnvelopeItem, Event, Log, Map},
};
use tracing_subscriber::{Registry, prelude::*};

static ENABLED: AtomicBool = AtomicBool::new(true);
const FLUSH_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn is_crash_reporter_process() -> bool {
    MinidumpIntegration::new().is_crash_reporter_process()
}

pub(crate) fn init() -> Option<ClientInitGuard> {
    let minidump = MinidumpIntegration::new().flush_timeout(FLUSH_TIMEOUT);
    // A reporter must enter init even if consent changed since its parent launched it.
    // Reading preferences takes no lock and never creates state.
    let enabled = !cfg!(feature = "simulator")
        && (minidump.is_crash_reporter_process() || Settings::telemetry_enabled_at_default_path());
    ENABLED.store(enabled, Ordering::SeqCst);
    let dsn = option_env!("SENTRY_DSN").filter(|dsn| !dsn.is_empty())?;
    if !enabled {
        return None;
    }
    let guard = sentry::init(
        options()
            .dsn(dsn)
            .transport(HttpTransport)
            .add_integration(minidump),
    );
    let _subscriber_already_set = tracing_subscriber::registry().with(layer()).try_init();
    Some(guard)
}

fn options() -> ClientOptions {
    ClientOptions::new()
        .attach_stacktrace(true)
        .release(concat!("workshop@", env!("CARGO_PKG_VERSION")))
        .environment(if cfg!(debug_assertions) {
            "development"
        } else {
            option_env!("SENTRY_ENVIRONMENT").unwrap_or("production")
        })
        .send_default_pii(false)
        .auto_session_tracking(true)
        .session_mode(sentry::SessionMode::Application)
        .shutdown_timeout(FLUSH_TIMEOUT)
        .traces_sampler(|context| {
            if ENABLED.load(Ordering::SeqCst)
                && matches!(context.operation(), "fiscal.receipt" | "fiscal.report")
            {
                1.0
            } else {
                0.0
            }
        })
        .before_send(scrub_event)
        .before_breadcrumb(scrub_breadcrumb)
        .before_send_log(scrub_log)
}

fn layer() -> SentryLayer<Registry> {
    sentry_tracing::layer()
        .event_filter(|metadata| match *metadata.level() {
            tracing::Level::ERROR => EventFilter::Event,
            tracing::Level::WARN | tracing::Level::INFO => {
                EventFilter::Breadcrumb | EventFilter::Log
            },
            tracing::Level::DEBUG | tracing::Level::TRACE => EventFilter::Ignore,
        })
        .span_filter(|metadata| metadata.fields().field("sentry.op").is_some())
}

fn scrub_event(mut event: Event<'static>) -> Option<Event<'static>> {
    if !ENABLED.load(Ordering::SeqCst) {
        return None;
    }
    event.message = Some(message(event.message.as_deref()).into());
    event.logentry = None;
    event.server_name = None;
    event.user = None;
    event.request = None;
    event.extra.clear();
    scrub_contexts(&mut event.contexts);
    event.culprit = None;
    event.transaction = event.transaction.map(|_| "fiscal.operation".into());
    event.logger = None;
    event
        .tags
        .retain(|key, _| matches!(key.as_str(), "printer_id" | "device_serial"));
    privacy::scrub_paths(&mut event);
    for exception in &mut event.exception.values {
        exception.value = None;
    }
    for breadcrumb in &mut event.breadcrumbs.values {
        scrub_breadcrumb_fields(breadcrumb);
    }
    Some(event)
}

fn scrub_breadcrumb(mut breadcrumb: Breadcrumb) -> Option<Breadcrumb> {
    if !ENABLED.load(Ordering::SeqCst) {
        return None;
    }
    scrub_breadcrumb_fields(&mut breadcrumb);
    Some(breadcrumb)
}

fn scrub_breadcrumb_fields(breadcrumb: &mut Breadcrumb) {
    breadcrumb.message = Some(message(breadcrumb.message.as_deref()).into());
    breadcrumb.category = None;
    breadcrumb.data.clear();
}

fn scrub_log(mut log: Log) -> Option<Log> {
    if !ENABLED.load(Ordering::SeqCst) {
        return None;
    }
    log.body = message(Some(&log.body)).into();
    log.attributes.retain(|key, _| {
        matches!(
            key.as_str(),
            "sentry.release" | "sentry.environment" | "printer_id" | "device_serial"
        )
    });
    Some(log)
}

pub(crate) fn set_enabled(enabled: bool) {
    ENABLED.store(enabled && !cfg!(feature = "simulator"), Ordering::SeqCst);
}

pub(crate) fn printer(printer_id: &str, serial: &str) {
    sentry::configure_scope(|scope| {
        scope.set_tag("printer_id", printer_id);
        scope.set_tag("device_serial", serial);
    });
    sentry::with_integration(|integration: &MinidumpIntegration, _| {
        integration.set_tag("printer_id".into(), Some(printer_id.into()));
        integration.set_tag("device_serial".into(), Some(serial.into()));
    });
}

struct HttpTransport;
impl TransportFactory for HttpTransport {
    fn create_transport_with_options(&self, options: TransportOptions) -> Arc<dyn Transport> {
        Arc::new(SafeTransport(
            transport::HttpTransport.create_transport_with_options(options),
        ))
    }
}

#[cfg(test)]
#[path = "telemetry/tests.rs"]
mod tests;

fn scrub_contexts(contexts: &mut Map<String, Context>) {
    contexts.retain(|_, context| {
        if let Context::Trace(trace) = context {
            trace.data.clear();
            trace.description = None;
            trace.op = trace.op.take().filter(|op| {
                matches!(
                    op.as_str(),
                    "fiscal.receipt" | "fiscal.report" | "http.client"
                )
            });
            true
        } else {
            false
        }
    });
}

struct SafeTransport(Arc<dyn Transport>);

impl Transport for SafeTransport {
    fn send_envelope(&self, envelope: Envelope) {
        if !ENABLED.load(Ordering::SeqCst) {
            return;
        }
        let mut safe = Envelope::new().with_headers(envelope.headers().clone());
        for mut item in envelope.into_items() {
            if let EnvelopeItem::Transaction(transaction) = &mut item {
                transaction.name = Some("fiscal.operation".into());
                transaction.user = None;
                transaction.server_name = None;
                transaction.request = None;
                transaction.extra.clear();
                transaction
                    .tags
                    .retain(|key, _| matches!(key.as_str(), "printer_id" | "device_serial"));
                scrub_contexts(&mut transaction.contexts);
                for span in &mut transaction.spans {
                    span.description = None;
                    span.data.clear();
                    span.tags.clear();
                }
            }
            safe.add_item(item);
        }
        self.0.send_envelope(safe);
    }

    fn flush(&self, timeout: Duration) -> bool {
        self.0.flush(timeout)
    }

    fn shutdown(&self, timeout: Duration) -> bool {
        self.0.shutdown(timeout)
    }
}

// Static, reviewed messages only. New instrumentation is redacted until explicitly admitted.
fn message(text: Option<&str>) -> &str {
    match text {
        Some(
            message @ ("Connected"
            | "Job claimed"
            | "Receipt printed"
            | "Report printed"
            | "Printer needs operator intervention"
            | "Printer requires operator intervention"
            | "Printer resumed after operator intervention"
            | "Printer refused command"
            | "Printer operation has an uncertain outcome"
            | "Fiscal receipt outcome is uncertain"
            | "Worker panicked"
            | "Worker operation requires attention"
            | "Worker operation failed"
            | "Holuto session unavailable; retrying in 15 seconds"
            | "Printed receipt conflicts with terminal Holuto state"
            | "Holuto rejected completed fiscal evidence"
            | "Operator acknowledged orphaned receipt; no command reissued"),
        ) => message,
        _ => "Fiscal application diagnostic",
    }
}
