use std::{
    error::Error,
    panic,
    sync::{Mutex, PoisonError},
};

use sentry::{
    Client, Hub,
    protocol::{ItemContainer, LogLevel},
};
use tracing::subscriber;

use super::*;

#[derive(Default)]
struct Capture(Mutex<Vec<Envelope>>);
impl Transport for Capture {
    fn send_envelope(&self, envelope: Envelope) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(envelope);
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn panic_and_error_have_printer_tags_and_no_sensitive_payload() -> Result<(), Box<dyn Error>> {
    let capture = Arc::new(Capture::default());
    let options = sentry::apply_defaults(options())
        .dsn("https://public@example.invalid/1")
        .transport(Arc::clone(&capture));
    let client = Arc::new(Client::from(options));
    let hub = Arc::new(Hub::new(Some(Arc::clone(&client)), Arc::default()));
    Hub::run(hub, || {
        printer("printer-42", "SERIAL42");
        let subscriber = tracing_subscriber::registry().with(layer());
        subscriber::with_default(subscriber, || {
            tracing::warn!(
                token = "SECRET_TOKEN",
                url = "https://host/private?key=secret",
                "Jan Kowalski 1234567890 Oil 250.00"
            );
            tracing::info!(line = "Oil", amount = "250.00", "SECRET_TOKEN");
            tracing::error!(
                customer = "Jan Kowalski",
                nip = "1234567890",
                "SECRET_TOKEN /private?key=secret Oil 250.00"
            );
        });
        let _caught =
            panic::catch_unwind(|| panic!("SECRET_TOKEN Jan Kowalski 1234567890 Oil 250.00"));
        assert!(
            client.flush(Some(Duration::from_secs(1))),
            "capturing transport flushes synchronously"
        );
    });
    let envelopes = capture.0.lock().map_err(|error| error.to_string())?;
    let events: Vec<_> = envelopes
        .iter()
        .flat_map(Envelope::items)
        .filter_map(|item| {
            if let EnvelopeItem::Event(event) = item {
                Some(event)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(events.len(), 2, "ERROR and panic both become issues");
    for event in events {
        assert_eq!(
            event.tags.get("printer_id").map(String::as_str),
            Some("printer-42"),
            "printer identifies the issue"
        );
        assert_eq!(
            event.tags.get("device_unique_number").map(String::as_str),
            Some("SERIAL42"),
            "physical device identifies the issue"
        );
        assert_eq!(
            event.breadcrumbs.values.len(),
            2,
            "WARN and INFO become breadcrumbs, not issues"
        );
    }
    let logs: Vec<_> = envelopes
        .iter()
        .flat_map(Envelope::items)
        .filter_map(|item| {
            if let EnvelopeItem::ItemContainer(ItemContainer::Logs(logs)) = item {
                Some(logs)
            } else {
                None
            }
        })
        .flatten()
        .map(|log| log.level)
        .collect();
    assert_eq!(
        logs,
        [LogLevel::Warn, LogLevel::Info],
        "WARN and INFO also become structured logs"
    );
    let mut bytes = Vec::new();
    for envelope in envelopes.iter() {
        envelope.to_writer(&mut bytes)?;
    }
    let payload = String::from_utf8(bytes)?;
    for private in [
        "SECRET_TOKEN",
        "Jan Kowalski",
        "1234567890",
        "Oil",
        "250.00",
        "/private",
        "key=secret",
    ] {
        assert!(!payload.contains(private), "telemetry leaked {private}");
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn disabling_telemetry_drops_all_signals_including_sessions_immediately()
-> Result<(), Box<dyn Error>> {
    let capture = Arc::new(Capture::default());
    let captured = Arc::clone(&capture);
    let transport = Arc::new(SafeTransport(captured));
    let client = Arc::new(Client::from(
        options()
            .dsn("https://public@example.invalid/1")
            .transport(transport),
    ));
    Hub::run(
        Arc::new(Hub::new(Some(Arc::clone(&client)), Arc::default())),
        || {
            ENABLED.store(false, Ordering::SeqCst);
            sentry::start_session();
            let _event = sentry::capture_message("private", sentry::Level::Error);
            sentry::add_breadcrumb(Breadcrumb::default());
            sentry::logger_info!("private");
            sentry::end_session();
            assert!(
                client.flush(Some(Duration::from_secs(1))),
                "disabled signals flush without sending"
            );
        },
    );
    assert!(
        capture
            .0
            .lock()
            .map_err(|error| error.to_string())?
            .is_empty(),
        "opt-out sends no envelope"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn only_fiscal_operations_are_sampled_and_transaction_data_is_scrubbed()
-> Result<(), Box<dyn Error>> {
    let capture = Arc::new(Capture::default());
    let captured = Arc::clone(&capture);
    let transport = Arc::new(SafeTransport(captured));
    let client = Arc::new(Client::from(
        options()
            .dsn("https://public@example.invalid/1")
            .transport(transport),
    ));
    Hub::run(
        Arc::new(Hub::new(Some(Arc::clone(&client)), Arc::default())),
        || {
            sentry::start_transaction(sentry::TransactionContext::new("poll", "http.client"))
                .finish();
            let transaction = sentry::start_transaction(sentry::TransactionContext::new(
                "SECRET_TOKEN",
                "fiscal.receipt",
            ));
            transaction.set_data("buyer_nip", "1234567890".into());
            transaction.finish();
            assert!(
                client.flush(Some(Duration::from_secs(1))),
                "transactions flush into the capture"
            );
        },
    );
    let envelopes = capture.0.lock().map_err(|error| error.to_string())?;
    let transactions: Vec<_> = envelopes
        .iter()
        .flat_map(Envelope::items)
        .filter_map(|item| {
            if let EnvelopeItem::Transaction(transaction) = item {
                Some(transaction)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        transactions.len(),
        1,
        "polling HTTP calls do not become standalone transactions"
    );
    assert_eq!(
        transactions
            .first()
            .and_then(|transaction| transaction.name.as_deref()),
        Some("fiscal.operation"),
        "transaction names cannot carry user input"
    );
    let mut bytes = Vec::new();
    for envelope in envelopes.iter() {
        envelope.to_writer(&mut bytes)?;
    }
    let payload = String::from_utf8(bytes)?;
    assert!(
        !payload.contains("SECRET_TOKEN") && !payload.contains("1234567890"),
        "transactions must not bypass the privacy boundary"
    );
    Ok(())
}
