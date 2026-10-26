use std::{
    error::Error,
    panic,
    sync::{Mutex, PoisonError},
};

use sentry::{
    Client, Hub,
    protocol::{ItemContainer, LogLevel, Request, User},
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
#[expect(
    clippy::too_many_lines,
    reason = "Keep error, breadcrumb and log canaries at the outgoing envelope boundary"
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
                host = "HOST_CANARY_9183",
                home = "/home/HOME_CANARY_9183",
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
    for event in envelopes
        .iter()
        .flat_map(Envelope::items)
        .filter_map(|item| {
            if let EnvelopeItem::Event(event) = item {
                Some(event)
            } else {
                None
            }
        })
    {
        assert_eq!(
            event.message.as_deref(),
            Some("Fiscal application diagnostic")
        );
        assert_eq!(
            event
                .breadcrumbs
                .values
                .iter()
                .map(|crumb| (
                    crumb.message.as_deref(),
                    crumb.category.as_deref(),
                    crumb.data.len()
                ))
                .collect::<Vec<_>>(),
            [
                (Some("Fiscal application diagnostic"), None, 0),
                (Some("Fiscal application diagnostic"), None, 0),
            ]
        );
    }
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
        "HOST_CANARY_9183",
        "HOME_CANARY_9183",
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
            let transaction = sentry::start_transaction(sentry::TransactionContext::new(
                "fiscal.operation",
                "fiscal.receipt",
            ));
            ENABLED.store(false, Ordering::SeqCst);
            sentry::start_session();
            let _event = sentry::capture_message("private", sentry::Level::Error);
            sentry::add_breadcrumb(Breadcrumb::default());
            sentry::logger_info!("private");
            sentry::end_session();
            transaction.finish();
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
            sentry::configure_scope(|scope| {
                scope.set_user(Some(User {
                    username: Some("CUSTOMER_CANARY".into()),
                    ..User::default()
                }));
                scope.set_tag("private_host", "HOST_CANARY");
                scope.set_context(
                    "private",
                    Context::Other(Map::from_iter([("host".into(), "HOST_CANARY".into())])),
                );
            });
            sentry::start_transaction(sentry::TransactionContext::new("poll", "http.client"))
                .finish();
            for operation in ["fiscal.receipt", "fiscal.report"] {
                let transaction = sentry::start_transaction(sentry::TransactionContext::new(
                    "SECRET_TOKEN",
                    operation,
                ));
                transaction.set_data("buyer_nip", "1234567890".into());
                transaction.set_extra("amount", "AMOUNT_CANARY".into());
                transaction.set_request(Request {
                    url: Some(
                        "https://HOST_CANARY/private?token=SECRET_TOKEN"
                            .parse()
                            .unwrap(),
                    ),
                    ..Request::default()
                });
                let span = transaction.start_child("http.client", "https://HOST_CANARY/private");
                span.set_data("customer", "CUSTOMER_CANARY".into());
                span.set_data("home", "/home/HOME_CANARY".into());
                span.set_data("amount", "AMOUNT_CANARY".into());
                span.finish();
                transaction.finish();
            }
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
        2,
        "polling HTTP calls do not become standalone transactions"
    );
    for transaction in transactions {
        assert_eq!(transaction.name.as_deref(), Some("fiscal.operation"));
        assert_eq!(transaction.user, None);
        assert_eq!(transaction.request, None);
        assert_eq!(transaction.server_name, None);
        assert!(transaction.tags.is_empty());
        assert_eq!(transaction.spans.len(), 1);
        for span in &transaction.spans {
            assert_eq!(span.op.as_deref(), Some("http.client"));
            assert_eq!(span.description, None);
            assert!(span.data.is_empty());
            assert!(span.tags.is_empty());
        }
    }
    let mut bytes = Vec::new();
    for envelope in envelopes.iter() {
        envelope.to_writer(&mut bytes)?;
    }
    let payload = String::from_utf8(bytes)?;
    for canary in [
        "SECRET_TOKEN",
        "1234567890",
        "CUSTOMER_CANARY",
        "AMOUNT_CANARY",
        "HOST_CANARY",
        "HOME_CANARY",
        "/private",
    ] {
        assert!(!payload.contains(canary), "transaction leaked {canary}");
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Capturing transports prove no envelope leaves a disabled SDK"
)]
fn absent_dsn_disables_errors_logs_sessions_and_fiscal_transactions() -> Result<(), Box<dyn Error>>
{
    let capture = Arc::new(Capture::default());
    let client = Arc::new(Client::from(options().transport(Arc::clone(&capture))));
    Hub::run(
        Arc::new(Hub::new(Some(Arc::clone(&client)), Arc::default())),
        || {
            sentry::start_session();
            let _event = sentry::capture_message("Worker operation failed", sentry::Level::Error);
            sentry::logger_warn!("Connected");
            sentry::start_transaction(sentry::TransactionContext::new(
                "fiscal.operation",
                "fiscal.receipt",
            ))
            .finish();
            sentry::end_session();
            assert!(client.flush(Some(Duration::from_secs(1))));
        },
    );
    assert!(
        capture
            .0
            .lock()
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The complete outgoing event is checked against synthetic private fields"
)]
fn event_allowlist_removes_nested_host_customer_request_and_scope_data()
-> Result<(), Box<dyn Error>> {
    let event: Event<'static> = serde_json::from_value(serde_json::json!({
        "message":"Worker operation failed",
        "server_name":"HOST_CANARY",
        "user":{"username":"CUSTOMER_CANARY","email":"CUSTOMER_CANARY@example.invalid"},
        "request":{"url":"https://HOST_CANARY/private?token=TOKEN_CANARY",
            "headers":{"Authorization":"Bearer TOKEN_CANARY"},"data":"NIP_CANARY"},
        "extra":{"nested":{"customer":"CUSTOMER_CANARY","amount":"AMOUNT_CANARY"}},
        "tags":{"printer_id":"printer-42","device_unique_number":"SERIAL42","host":"HOST_CANARY"},
        "contexts":{"private":{"customer":"CUSTOMER_CANARY"},
            "trace":{"trace_id":"00000000000000000000000000000001",
                "span_id":"0000000000000001","op":"fiscal.receipt",
                "description":"TOKEN_CANARY","data":{"nip":"NIP_CANARY"}}},
        "breadcrumbs":{"values":[{"message":"TOKEN_CANARY","category":"HOST_CANARY",
            "data":{"customer":"CUSTOMER_CANARY","nip":"NIP_CANARY"}}]},
        "exception":{"values":[{"type":"PrinterFailure","value":"TOKEN_CANARY"}]}
    }))?;
    let capture = Arc::new(Capture::default());
    let client = Arc::new(Client::from(
        options()
            .dsn("https://public@example.invalid/1")
            .transport(Arc::clone(&capture)),
    ));
    Hub::run(Arc::new(Hub::new(Some(client), Arc::default())), || {
        let _event = sentry::capture_event(event);
    });
    let envelopes = capture.0.lock().map_err(|error| error.to_string())?;
    let events = envelopes
        .iter()
        .flat_map(Envelope::items)
        .filter_map(|item| {
            if let EnvelopeItem::Event(event) = item {
                Some(event)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let [event] = events.as_slice() else {
        return Err("expected one captured event".into());
    };
    assert_eq!(event.message.as_deref(), Some("Worker operation failed"));
    assert_eq!(
        event.tags,
        Map::from_iter([
            ("printer_id".into(), "printer-42".into()),
            ("device_unique_number".into(), "SERIAL42".into()),
        ])
    );
    let mut bytes = Vec::new();
    for envelope in envelopes.iter() {
        envelope.to_writer(&mut bytes)?;
    }
    let payload = String::from_utf8(bytes)?;
    for canary in [
        "HOST_CANARY",
        "CUSTOMER_CANARY",
        "TOKEN_CANARY",
        "NIP_CANARY",
        "AMOUNT_CANARY",
        "/private",
    ] {
        assert!(!payload.contains(canary), "event leaked {canary}");
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn fixed_driver_and_worker_warnings_reach_sentry_with_their_meaning() -> Result<(), Box<dyn Error>>
{
    let capture = Arc::new(Capture::default());
    let client = Arc::new(Client::from(
        options()
            .dsn("https://public@example.invalid/1")
            .transport(Arc::clone(&capture)),
    ));
    Hub::run(Arc::new(Hub::new(Some(client), Arc::default())), || {
        let subscriber = tracing_subscriber::registry().with(layer());
        subscriber::with_default(subscriber, || {
            // The exact calls in the Posnet driver and the worker.
            tracing::warn!(
                command = "trend",
                "Receipt footer incomplete after the fiscal record"
            );
            tracing::warn!("Could not load the receipt display history");
            tracing::error!("Worker operation failed");
        });
    });
    let envelopes = capture.0.lock().map_err(|error| error.to_string())?;
    let breadcrumbs: Vec<_> = envelopes
        .iter()
        .flat_map(Envelope::items)
        .filter_map(|item| {
            if let EnvelopeItem::Event(event) = item {
                Some(event)
            } else {
                None
            }
        })
        .flat_map(|event| event.breadcrumbs.values.iter())
        .map(|crumb| crumb.message.as_deref())
        .collect();
    assert_eq!(
        breadcrumbs,
        [
            Some("Receipt footer incomplete after the fiscal record"),
            Some("Could not load the receipt display history"),
        ]
    );
    Ok(())
}

#[test]
fn safe_transport_shutdown_preserves_the_underlying_outcome_and_deadline() {
    struct Shutdown {
        result: bool,
        calls: Mutex<Vec<Duration>>,
    }
    impl Transport for Shutdown {
        fn send_envelope(&self, _envelope: Envelope) {}

        fn shutdown(&self, timeout: Duration) -> bool {
            self.calls.lock().unwrap().push(timeout);
            self.result
        }
    }
    for result in [false, true] {
        let capture = Arc::new(Shutdown {
            result,
            calls: Mutex::new(Vec::new()),
        });
        let captured = Arc::clone(&capture);
        let transport = SafeTransport(captured);
        assert_eq!(transport.shutdown(Duration::from_millis(37)), result);
        assert_eq!(*capture.calls.lock().unwrap(), [Duration::from_millis(37)]);
    }
}

#[test]
fn consent_changes_gate_outgoing_events_and_simulators_never_enable_capture() {
    let capture = Arc::new(Capture::default());
    let client = Arc::new(Client::from(
        options()
            .dsn("https://public@example.invalid/1")
            .transport(Arc::clone(&capture)),
    ));
    Hub::run(Arc::new(Hub::new(Some(client), Arc::default())), || {
        set_enabled(false);
        let _event = sentry::capture_message("Worker operation failed", sentry::Level::Error);
        assert!(capture.0.lock().unwrap().is_empty());
        set_enabled(true);
        let _event = sentry::capture_message("Worker operation failed", sentry::Level::Error);
        let envelopes = capture.0.lock().unwrap();
        let messages = envelopes
            .iter()
            .flat_map(Envelope::items)
            .filter_map(|item| {
                if let EnvelopeItem::Event(event) = item {
                    event.message.as_deref()
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if cfg!(feature = "simulator") {
            assert_eq!(messages, Vec::<&str>::new());
        } else {
            assert_eq!(messages, ["Worker operation failed"]);
        }
    });
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Reporter detection must work before any client initialization"
)]
fn native_reporter_marker_is_recognized_in_an_isolated_child() -> Result<(), Box<dyn Error>> {
    use std::{env, fs, process::Command};
    const CHILD: &str = "WORKSHOP_REPORTER_PREDICATE_CHILD";
    if let Some(mode) = env::var_os(CHILD) {
        let reporter = is_crash_reporter_process();
        assert_eq!(reporter, mode == "reporter");
        fs::write(
            env::var_os("WORKSHOP_REPORTER_OUTCOME").ok_or("missing outcome path")?,
            reporter.to_string(),
        )?;
        return Ok(());
    }
    let temporary = tempfile::tempdir()?;
    for (mode, expected) in [("app", "false"), ("reporter", "true")] {
        let outcome = temporary.path().join(mode);
        #[expect(
            clippy::disallowed_methods,
            reason = "SDK reporter environment is child-local"
        )]
        let mut child = Command::new(env::current_exe()?);
        let _child = child
            .args([
                "--exact",
                "telemetry::tests::native_reporter_marker_is_recognized_in_an_isolated_child",
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env("WORKSHOP_REPORTER_OUTCOME", &outcome);
        // https://github.com/getsentry/sentry-rust/blob/0.49.3/sentry-minidump/src/lib.rs
        if mode == "reporter" {
            let _child = child.env("_SENTRY_CRASH_REPORTER_SERVER", "synthetic-marker-only");
        } else {
            let _child = child.env_remove("_SENTRY_CRASH_REPORTER_SERVER");
        }
        let output = child.output()?;
        assert!(output.status.success(), "{output:?}");
        assert_eq!(fs::read_to_string(outcome)?, expected);
    }
    Ok(())
}

/// Runs the real native reporter in a crashing child.
///
/// The SDK re-executes the child as the reporter, which acknowledges the crash only after
/// `before_capture` has run, so the crashing child exits after the outcome is written.
#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn native_crash_dump_is_written_inside_the_private_state_directory() -> Result<(), Box<dyn Error>> {
    use std::{
        env, fs,
        os::unix::fs::PermissionsExt as _,
        path::PathBuf,
        process::{self, Command},
    };
    const CHILD: &str = "WORKSHOP_CRASH_DUMP_CHILD";
    const STATE: &str = "WORKSHOP_CRASH_DUMP_STATE";
    const OUTCOME: &str = "WORKSHOP_CRASH_DUMP_OUTCOME";
    if env::var_os(CHILD).is_some() {
        let state = PathBuf::from(env::var_os(STATE).ok_or("missing state path")?);
        let outcome = PathBuf::from(env::var_os(OUTCOME).ok_or("missing outcome path")?);
        let integration = minidump(&state).before_capture(move |_scope, dump| {
            // Runs in the reporter process while the dump exists on disk.
            let written = dump.is_file().then(|| dump.display().to_string());
            let _written = fs::write(&outcome, written.unwrap_or_default());
        });
        // In the re-executed reporter, `init` serves the crash and exits the process.
        let _guard = sentry::init(
            options()
                .dsn("https://public@example.invalid/1")
                .transport(Arc::new(Capture::default()))
                .add_integration(integration),
        );
        process::abort();
    }
    let temporary = tempfile::tempdir()?;
    let state = StateDir::open(temporary.path().join("state"))?;
    let shared = temporary.path().join("shared-tmp");
    fs::create_dir(&shared)?;
    let outcome = temporary.path().join("outcome");
    #[expect(
        clippy::disallowed_methods,
        reason = "Reporter and temporary-directory environment is child-local"
    )]
    let mut child = Command::new(env::current_exe()?);
    let output = child
        .args([
            "--exact",
            "telemetry::tests::native_crash_dump_is_written_inside_the_private_state_directory",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(STATE, state.path())
        .env(OUTCOME, &outcome)
        // The SDK default would put the dump in `$TMPDIR/Crashes`.
        .env("TMPDIR", &shared)
        .env_remove("_SENTRY_CRASH_REPORTER_SERVER")
        .output()?;
    assert!(!output.status.success(), "the child must crash: {output:?}");
    let dump = PathBuf::from(fs::read_to_string(&outcome)?);
    assert_eq!(
        dump.parent(),
        Some(state.path().join("crashes").as_path()),
        "dump written outside the state directory"
    );
    assert!(
        !shared.join("Crashes").exists(),
        "dump written to the shared temporary directory"
    );
    assert_eq!(
        fs::metadata(state.path())?.permissions().mode() & 0o777,
        0o700,
        "only the user can reach the dump"
    );
    Ok(())
}
