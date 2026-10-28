#[cfg(target_os = "linux")]
#[path = "tests/autostart.rs"]
mod autostart;
#[path = "tests/end_to_end.rs"]
mod end_to_end;
// Unit tests never run on AppKit's main thread, so the macOS quit hook fails closed.
#[cfg(not(target_os = "macos"))]
#[path = "tests/pairing.rs"]
mod pairing;
#[cfg(unix)]
#[path = "tests/signals.rs"]
mod signals;
#[cfg(not(target_os = "macos"))]
#[path = "tests/start.rs"]
mod start;

use std::{
    error::Error,
    fs,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use agent::{
    api::model::JobView,
    documents::{Document, ReceiptKind},
};
use fiscal::device::{Device, Vendor};
use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use jiff::Timestamp;
use posnet::{capabilities::capabilities, simulator::Simulator};
use sentry::{Client, ClientOptions, Envelope, Hub, Level, Transport, protocol::EnvelopeItem};
use slint::platform::{Key, WindowEvent};
use tempfile::TempDir;
use uuid::Uuid;

use super::*;

type UiFixture = (TempDir, Rc<RefCell<Application>>, AppWindow);

fn fixture() -> Result<UiFixture, Box<dyn Error>> {
    i_slint_backend_testing::init_no_event_loop();
    fixture_data()
}

fn fixture_with_event_loop() -> Result<UiFixture, Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    fixture_data()
}

fn fixture_data() -> Result<UiFixture, Box<dyn Error>> {
    let temp = TempDir::new()?;
    let simulator = Simulator::start();
    let app = make_app(
        Arc::new(StateDir::open(temp.path().to_owned())?),
        Settings::default(),
        simulator.link(),
    )?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing app window")?
        .clone_strong();
    window
        .window()
        .set_size(slint::LogicalSize::new(1180.0, 820.0));
    Ok((temp, app, window))
}

fn make_app(
    dir: Arc<StateDir>,
    settings: Settings,
    link: Arc<dyn Link>,
) -> Result<Rc<RefCell<Application>>, slint::PlatformError> {
    let app = Rc::new(RefCell::new(Application::new(dir, settings, link, || {
        Date::constant(2025, 10, 24)
    })));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    app.borrow_mut().launch()?;
    Ok(app)
}

fn control(
    window: &AppWindow,
    label: &str,
    role: AccessibleRole,
) -> Result<ElementHandle, Box<dyn Error>> {
    let elements = ElementHandle::find_by_accessible_label(window, label)
        .filter(|element| element.accessible_role() == Some(role))
        .collect::<Vec<_>>();
    match elements.as_slice() {
        [only] => Ok(only.clone()),
        _ => Err(format!(
            "expected one accessible control {label}, found {}",
            elements.len()
        )
        .into()),
    }
}
fn button(window: &AppWindow, label: &str) -> Result<ElementHandle, Box<dyn Error>> {
    control(window, label, AccessibleRole::Button)
}
fn ready(app: &Rc<RefCell<Application>>) {
    let mut app = app.borrow_mut();
    app.event(Event::Connected {
        printer_id: "printer-42".into(),
        unique_number: "TST0000000042".into(),
        vendor: Vendor::Posnet,
        port: "test".into(),
    });
    app.event(Event::Device {
        status: Some(Device {
            vendor: Vendor::Posnet,
            model: "TEMO".into(),
            firmware: "32.01".into(),
            unique_number: "TST0000000042".into(),
            problems: Vec::new(),
            vat: Vec::new(),
            capabilities: capabilities(),
        }),
        blocked: false,
        report_unresolved: false,
        orphaned_receipt: None,
    });
    app.event(Event::Documents {
        items: vec![Document {
            kind: ReceiptKind::Sale,
            order_id: Uuid::nil(),
            order_number: 42,
            allocation_id: None,
            amount: 25_000,
            customer_name: Some("Jan Kowalski".into()),
            occurred_at: Timestamp::UNIX_EPOCH,
            due_at: Timestamp::UNIX_EPOCH,
            latest_at: Timestamp::UNIX_EPOCH,
            is_overdue: false,
        }],
        page: 1,
        has_previous: false,
        has_next: true,
    });
    app.model.selected = Some(0);
    app.model.manual = true;
    app.sync();
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert accessible application behavior"
)]
fn receipts_are_the_default_and_manual_documents_open_in_a_dialog() -> Result<(), Box<dyn Error>> {
    let (_temporary, _app, window) = fixture()?;
    assert_eq!(window.get_tab(), 0);
    let _search = control(&window, "Szukaj", AccessibleRole::TextInput)?;
    button(&window, "+ Wystaw paragon")?.invoke_accessible_default_action();
    let _documents_search = control(&window, "Szukaj dokumentu", AccessibleRole::TextInput)?;
    let _next = button(&window, "Dalej")?;
    button(&window, "Zamknij")?.invoke_accessible_default_action();
    assert!(
        ElementHandle::find_by_accessible_label(&window, "Szukaj dokumentu")
            .next()
            .is_none()
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn queue_confirmation_shows_amount_nip_and_local_pending_state() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    button(&window, "Dalej")?.invoke_accessible_default_action();
    control(&window, "NIP nabywcy", AccessibleRole::TextInput)?.set_accessible_value("1234567890");
    assert_eq!(
        window.get_confirm_text().as_str(),
        "Sprzedaż · zlecenie 42 · Jan Kowalski · 250,00 zł\nDrukarka wystawi paragon fiskalny.",
        "operator sees the binding amount"
    );
    let confirm = ElementHandle::find_by_accessible_label(&window, "Wystaw paragon")
        .filter(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .last()
        .ok_or("missing confirmation button")?;
    confirm.invoke_accessible_default_action();
    assert!(!window.get_manual(), "queued receipts return to Paragony");
    button(&window, "+ Wystaw paragon")?.invoke_accessible_default_action();
    assert_eq!(
        button(&window, "Dalej")?.accessible_enabled(),
        Some(false),
        "confirmed receipt waits for worker acknowledgement"
    );
    assert_eq!(
        window.get_nip().as_str(),
        "",
        "a submitted buyer NIP is not carried over to the next receipt"
    );
    app.borrow_mut().event(Event::CommandCompleted);
    app.borrow().sync();
    assert_eq!(
        button(&window, "Dalej")?.accessible_enabled(),
        Some(true),
        "completion restores receipt action"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn paging_buttons_reflect_the_worker_page() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    let documents = |page, has_previous, has_next| Event::Documents {
        items: Vec::new(),
        page,
        has_previous,
        has_next,
    };
    // Server cursors, not the local page number, decide availability: page 1 may go back.
    for (event, feed, expected) in [
        (documents(1, false, true), "", (false, true)),
        (documents(1, true, false), "", (true, false)),
        (documents(2, true, false), "", (true, false)),
        (
            Event::Jobs {
                items: Vec::new(),
                page: 1,
                has_previous: true,
                has_next: false,
            },
            " historii",
            (true, false),
        ),
    ] {
        app.borrow_mut().event(event);
        app.borrow().sync();
        let enabled = |direction| {
            button(&window, &format!("{direction} strona{feed}"))
                .map(|button| button.accessible_enabled())
        };
        assert_eq!(
            (enabled("Poprzednia")?, enabled("Następna")?),
            (Some(expected.0), Some(expected.1)),
            "paging{feed} for {expected:?}"
        );
    }
    assert_eq!(
        window.get_page_label().as_str(),
        "Strona 2",
        "operator sees current page"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn launch_publishes_an_unresolved_journal_block_before_processing_starts()
-> Result<(), Box<dyn Error>> {
    // An event loop lets launch install its session-end handler, as in production.
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temp = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temp.path().to_owned())?);
    let interrupted = r#"{
  "report": {
    "report": {"kind": "periodic", "from": "2026-09-01", "to": "2026-09-30", "summary": false},
    "vendor": "posnet",
    "device_unique_number": "POS0000123456",
    "started_at": "2026-10-16T10:00:00Z",
    "device": {"dispatching": {"stage": "periodicrepbydates"}},
    "acknowledged_at": null
  }
}"#;
    fs::write(temp.path().join("operation.json"), interrupted)?;
    let simulator = Simulator::start();
    let app = make_app(dir, Settings::default(), simulator.link())?;
    let published = app
        .borrow()
        .status_requested
        .clone()
        .ok_or("launch published no status")?;
    assert_eq!(
        (
            published.processing,
            published.blocked,
            published.report_unresolved,
            published.orphaned_receipt
        ),
        (false, true, true, false),
        "`workshop status` must not report an unresolved report as unblocked"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("operation.json"))?,
        interrupted,
        "launch never rewrites the fiscal journal"
    );
    // On macOS the unavailable quit hook's error replaces this explanation in tests.
    if cfg!(not(target_os = "macos")) {
        // Save feedback stays hidden while blocked, so launch must explain the block itself.
        let explanation = concat!(
            "Poprzednia operacja jest nierozstrzygnięta. ",
            "Uruchom obsługę drukarki, aby ją wyjaśnić."
        );
        assert_eq!(
            app.borrow().model.message,
            explanation,
            "launch explains the block instead of the pairing prompt"
        );
        let settings = app.borrow().model.settings.clone();
        app.borrow_mut()
            .settings_saved(Ok(preferences::Outcome::Saved(settings)));
        assert_eq!(
            app.borrow().model.message,
            explanation,
            "a later save keeps the block explanation visible"
        );
    }
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn a_restarted_worker_starts_without_the_previous_page_or_documents() -> Result<(), Box<dyn Error>>
{
    let (_temp, app, window) = fixture()?;
    ready(&app);
    let mut state = app.borrow_mut();
    let items = state.model.documents().to_vec();
    state.event(Event::Documents {
        items,
        page: 3,
        has_previous: true,
        has_next: true,
    });
    state.event(Event::Jobs {
        items: vec![receipt_view(10, "done")?],
        page: 2,
        has_previous: true,
        has_next: true,
    });
    state.event(Event::Stopped);
    state.event(Event::Connected {
        printer_id: "printer-42".into(),
        unique_number: "TST0000000042".into(),
        vendor: Vendor::Posnet,
        port: "test".into(),
    });
    state.sync();
    drop(state);
    assert_eq!(window.get_page_label().as_str(), "Strona 1");
    assert!(
        ElementHandle::find_by_accessible_label(&window, "Poprzednia strona")
            .next()
            .is_none(),
        "the new session has no other page, so no paging is offered"
    );
    assert!(
        app.borrow().model.documents().is_empty(),
        "documents from the stopped session are not offered for printing"
    );
    assert_eq!(
        button(&window, "Dalej")?.accessible_enabled(),
        Some(false),
        "no stale selection can be queued"
    );
    assert_eq!(
        (
            window.get_receipts().row_count(),
            window.get_receipt_detail().order.to_string(),
            window.get_receipt_selected(),
            window.get_jobs_previous_enabled(),
            window.get_jobs_next_enabled()
        ),
        (0, String::new(), -1, false, false),
        "stopped sessions never retain server receipt rows, details or paging"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn a_printer_of_another_brand_never_publishes_the_previous_printers_vendor()
-> Result<(), Box<dyn Error>> {
    let (_temp, app, _window) = fixture()?;
    ready(&app);
    let mut state = app.borrow_mut();
    state.event(Event::Stopped);
    state.event(Event::PortDetected {
        port: "novitus".into(),
        vendor: Vendor::Novitus,
    });
    state.event(Event::Connected {
        printer_id: "printer-42".into(),
        unique_number: "NOV0000000001".into(),
        vendor: Vendor::Novitus,
        port: "novitus".into(),
    });
    let status = state.model.status(Timestamp::UNIX_EPOCH);
    assert_eq!(
        (status.vendor, status.unique_number.as_deref()),
        (Some(Vendor::Novitus), Some("NOV0000000001")),
        "a connected status names the connected printer's brand"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn opening_the_port_list_shows_a_printer_plugged_in_after_launch() -> Result<(), Box<dyn Error>> {
    use fiscal::{
        error::Error as LinkError,
        link::{Candidate, Port},
    };

    /// Enumerates like `SerialLink`; nothing in this test may open a port.
    #[derive(Debug, Default)]
    struct Hotplug(Mutex<Vec<Candidate>>);
    impl Link for Hotplug {
        fn open(&self, port: &str, _baud_rate: u32) -> Result<Box<dyn Port>, LinkError> {
            Err(LinkError::Invalid(format!("unexpected open of {port}")))
        }

        fn candidates(&self) -> Vec<Candidate> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let link = Arc::new(Hotplug::default());
    let app = make_app(
        Arc::new(StateDir::open(temporary.path().to_owned())?),
        Settings {
            port: "/dev/ttyUNKNOWN".into(),
            ..Settings::default()
        },
        Arc::<Hotplug>::clone(&link),
    )?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing app window")?
        .clone_strong();
    link.0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Candidate {
            port: "plugged-after-launch".into(),
            hint: Some(Vendor::Posnet),
        });
    window.set_tab(2);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    app.borrow().sync();
    assert_eq!(
        window.get_ports().iter().collect::<Vec<_>>(),
        [
            "Automatyczne wykrywanie",
            "/dev/ttyUNKNOWN",
            "plugged-after-launch"
        ],
        "showing Ustawienia lists a printer connected while the app was running, \
         and the saved port stays offered even when enumeration skips it"
    );
    assert_eq!(window.get_port_index(), 1, "the saved port stays selected");
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn stopped_worker_keeps_errors() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    let mut state = app.borrow_mut();
    state.model.running = true;
    state.model.stopping = true;
    state.event(Event::Problem(Some("Błąd: brak połączenia".into())));
    state.event(Event::Stopped);
    state.sync();
    assert_eq!(
        window.get_status().as_str(),
        "Błąd: brak połączenia",
        "a worker failure stays visible after it stops"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn all_clear_hides_the_problem_footer() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    app.borrow_mut()
        .event(Event::Problem(Some("Błąd: brak połączenia".into())));
    app.borrow().sync();
    let footer = ElementHandle::find_by_accessible_label(&window, "Stan operacji")
        .next()
        .ok_or("missing problem footer")?;
    assert_eq!(
        footer.accessible_value().as_deref(),
        Some("Błąd: brak połączenia")
    );
    app.borrow_mut().event(Event::Problem(None));
    app.borrow().sync();
    assert_eq!(window.get_status(), "");
    assert!(
        ElementHandle::find_by_accessible_label(&window, "Stan operacji")
            .next()
            .is_none(),
        "all clear removes the footer from the accessible application"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn issues_after_stop_carry_no_previous_printer_identity() -> Result<(), Box<dyn Error>> {
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
    let (_temp, app, _window) = fixture()?;
    let capture = Arc::new(Capture::default());
    let client = Arc::new(Client::from(
        ClientOptions::new()
            .dsn("https://public@example.invalid/1")
            .transport(Arc::clone(&capture)),
    ));
    let hub = Arc::new(Hub::new(Some(Arc::clone(&client)), Arc::default()));
    Hub::run(hub, || {
        ready(&app);
        app.borrow_mut().event(Event::Stopped);
        // E.g. the next worker fails on another printer before it connects.
        let _id = sentry::capture_message("Worker operation failed", Level::Error);
        assert!(client.flush(Some(Duration::from_secs(1))));
    });
    let envelopes = capture.0.lock().map_err(|error| error.to_string())?;
    let event = envelopes
        .iter()
        .flat_map(Envelope::items)
        .find_map(|item| {
            if let EnvelopeItem::Event(event) = item {
                Some(event)
            } else {
                None
            }
        })
        .ok_or("missing issue")?;
    assert!(
        !event.tags.contains_key("printer_id") && !event.tags.contains_key("device_unique_number"),
        "a failure after Stop must not be attributed to the stopped printer"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn reload_after_pairing_keeps_a_diagnostics_choice_made_meanwhile() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture_with_event_loop()?;
    window.set_tab(2);
    // `Paired` submits Reload; the opt-out is written only after Reload has read the file.
    assert!(
        app.borrow_mut()
            .write_settings(preferences::Request::Reload)
    );
    control(&window, "Udostępniaj diagnostykę", AccessibleRole::Checkbox)?
        .invoke_accessible_default_action();
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    assert!(
        !app.borrow().model.settings.telemetry_enabled,
        "the reloaded settings must not restore diagnostics, which a later Save would persist"
    );
    assert!(
        !window.get_telemetry_enabled(),
        "checkbox shows the opt-out"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn report_input_before_2000_shows_error_instead_of_confirmation() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    window.set_tab(1);
    control(&window, "Data początkowa", AccessibleRole::TextInput)?
        .set_accessible_value("31.12.1999");
    control(&window, "Data końcowa", AccessibleRole::TextInput)?.set_accessible_value("01.01.2000");
    button(&window, "Drukuj raport okresowy")?.invoke_accessible_default_action();
    assert_eq!(
        window.get_status().as_str(),
        "Data początkowa nie może być wcześniejsza niż 01.01.2000.",
        "operator can correct the invalid date"
    );
    assert!(
        !window.get_confirming(),
        "invalid report cannot reach confirmation"
    );
    Ok(())
}

fn paired_fixture() -> Result<UiFixture, Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temp = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temp.path().to_owned())?);
    let settings = Settings {
        api_url: "https://paired.example.test".into(),
        token: "already-paired-secret".into(),
        ..Settings::default()
    };
    settings.save(&dir)?;
    let app = make_app(dir, settings, Simulator::start().link())?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing app window")?
        .clone_strong();
    window.set_tab(2);
    Ok((temp, app, window))
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn a_status_write_acknowledgement_keeps_settings_typed_meanwhile() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture_with_event_loop()?;
    window.set_tab(2);
    control(&window, "Kod parowania", AccessibleRole::TextInput)?.set_accessible_value("ABCD-1234");
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    assert_eq!(
        window.get_pairing_code().as_str(),
        "ABCD-1234",
        "the launch status acknowledgement must not overwrite operator input"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn quit_completes_when_the_final_status_cannot_be_submitted() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temp = TempDir::new()?;
    let simulator = Simulator::start();
    let app = Rc::new(RefCell::new(Application::new(
        Arc::new(StateDir::open(temp.path().to_owned())?),
        Settings::default(),
        simulator.link(),
        || Date::constant(2025, 10, 24),
    )));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    app.borrow_mut().settings_writer = Some(preferences::Writer::closed());
    app.borrow_mut().launch()?;
    let _scheduled = slint::invoke_from_event_loop(|| {
        with_app(|app| {
            app.event(Event::Device {
                status: None,
                blocked: true,
                report_unresolved: false,
                orphaned_receipt: None,
            });
            app.stop(true)
        });
    });
    assert!(
        quit_before_deadline()?,
        "a status write that was never submitted must not keep quit waiting"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn an_older_status_acknowledgement_never_replaces_a_newer_unsubmitted_status()
-> Result<(), Box<dyn Error>> {
    // Launch's acknowledgement stays queued until after the newer submission fails.
    let (_temp, app, _window) = fixture_with_event_loop()?;
    app.borrow_mut().settings_writer = Some(preferences::Writer::closed());
    app.borrow_mut().event(Event::Device {
        status: None,
        blocked: true,
        report_unresolved: false,
        orphaned_receipt: None,
    });
    app.borrow_mut().stop(true)?;
    assert!(
        quit_before_deadline()?,
        "the stale launch acknowledgement must not keep quit waiting"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn a_start_whose_settings_cannot_be_submitted_can_be_retried() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = paired_fixture()?;
    app.borrow_mut().settings_writer = Some(preferences::Writer::closed());
    button(&window, "Uruchom")?.invoke_accessible_default_action();
    assert!(
        app.borrow()
            .model
            .message
            .starts_with("Nie można zapisać ustawień")
    );
    assert_eq!(
        button(&window, "Uruchom")?.accessible_enabled(),
        Some(true),
        "no completion will arrive, so Start must not stay pending"
    );
    Ok(())
}

/// Returns false if the deadline forced the event loop to quit.
fn quit_before_deadline() -> Result<bool, Box<dyn Error>> {
    let (finished, quit) = mpsc::channel::<()>();
    let deadline = thread::Builder::new()
        .name("quit deadline".into())
        .spawn(move || {
            let hung = quit.recv_timeout(Duration::from_secs(5)).is_err();
            if hung {
                let _scheduled = slint::invoke_from_event_loop(|| {
                    let _quit = slint::quit_event_loop();
                });
            }
            hung
        })?;
    slint::run_event_loop_until_quit()?;
    let _sent = finished.send(());
    let hung = deadline
        .join()
        .map_err(|_panic| "quit deadline thread panicked")?;
    Ok(!hung)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn nested_ui_callback_is_drained_without_refcell_panic() -> Result<(), Box<dyn Error>> {
    let (_temp, _app, window) = fixture()?;
    with_app(|app| {
        with_app(|app| {
            app.model.problem("Nested callback failed".into());
            Ok(())
        });
        app.model.problem("Outer callback failed".into());
        Ok(())
    });
    assert_eq!(
        window.get_status().as_str(),
        "Nested callback failed",
        "reentrant action executes after outer mutation"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn orphaned_receipt_acknowledgement_requires_explicit_confirmation() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    let mut state = app.borrow_mut();
    state.event(Event::Device {
        status: None,
        blocked: true,
        report_unresolved: false,
        orphaned_receipt: Some("Paragon 17 został wydrukowany dla zlecenia 42".into()),
    });
    // A later error must not replace the evidence the acknowledgement quotes.
    state.event(Event::Problem(Some("Błąd: Holuto niedostępne".into())));
    state.sync();
    drop(state);
    button(&window, "Wyjaśnij paragon")?.invoke_accessible_default_action();
    assert_eq!(
        window.get_confirm_text(),
        concat!(
            "Paragon 17 został wydrukowany dla zlecenia 42\n\n",
            "Sprawdź wydruk i uzgodnij dokument w Holuto. ",
            "Potwierdzenie zapisze lokalne wyjaśnienie i usunie blokadę. ",
            "Nie wydrukuje paragonu ponownie ani nie zmieni wyniku w Holuto."
        )
    );
    button(&window, "Anuluj")?.invoke_accessible_default_action();
    assert!(
        !window.get_confirming() && window.get_orphaned_receipt(),
        "cancelling closes the dialog without acknowledging"
    );
    button(&window, "Wyjaśnij paragon")?.invoke_accessible_default_action();
    button(&window, "Potwierdź wyjaśnienie")?.invoke_accessible_default_action();
    assert!(
        !window.get_orphaned_receipt(),
        "confirming submits the acknowledgement"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn accessible_headers_sort_visible_rows_without_changing_the_selected_document()
-> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    let mut documents = app.borrow().model.documents().to_vec();
    let mut other = documents.first().ok_or("missing fixture document")?.clone();
    other.order_id = Uuid::from_u128(1);
    other.order_number = 1;
    documents.push(other);
    app.borrow_mut().event(Event::Documents {
        items: documents,
        page: 1,
        has_previous: false,
        has_next: false,
    });
    app.borrow().sync();
    button(&window, "Zlecenie")?.invoke_accessible_default_action();
    assert_eq!(
        window
            .get_documents()
            .row_data(0)
            .and_then(|row| row.row_data(0))
            .map(|cell| cell.text.to_string()),
        Some("1".into()),
        "ascending header sorts the rows the operator sees"
    );
    control(&window, "42", AccessibleRole::ListItem)?.invoke_accessible_default_action();
    button(&window, "Zlecenie")?.invoke_accessible_default_action();
    assert_eq!(
        window
            .get_documents()
            .row_data(0)
            .and_then(|row| row.row_data(0))
            .map(|cell| cell.text.to_string()),
        Some("42".into()),
        "second header action sorts descending"
    );
    button(&window, "Dalej")?.invoke_accessible_default_action();
    assert_eq!(
        window.get_confirm_text(),
        "Sprzedaż · zlecenie 42 · Jan Kowalski · 250,00 zł\nDrukarka wystawi paragon fiskalny.",
        "sorting must never move selection to another document"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn close_without_a_tray_host_keeps_the_window_visible() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    app.borrow_mut().close_after_probe(false);
    assert!(
        window.window().is_visible(),
        "closing without a tray must not strand the operator"
    );
    assert_eq!(
        app.borrow().model.message,
        "Zasobnik systemowy jest niedostępny. Aby zakończyć aplikację, kliknij Zakończ.",
        "operator gets an explicit exit route"
    );
    Ok(())
}

// Slint's AppKit tray requires the main thread, which unit tests never run on.
#[cfg(not(target_os = "macos"))]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn close_with_a_live_tray_hides_and_reopening_restores_the_window() -> Result<(), Box<dyn Error>> {
    use slint::language::SortOrder;

    let (_temp, app, window) = fixture()?;
    ready(&app);
    button(&window, "Zlecenie")?.invoke_accessible_default_action();
    app.borrow_mut().tray = Some(TrayIcon::new()?);
    app.borrow_mut().close_after_probe(true);
    assert!(
        !window.window().is_visible(),
        "tray close hides the visible window"
    );
    assert!(
        app.borrow().window.is_none(),
        "tray close releases the component"
    );
    app.borrow_mut().open()?;
    let reopened = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing reopened window")?
        .clone_strong();
    assert!(reopened.window().is_visible());
    assert_eq!(reopened.get_selected(), 0);
    assert_eq!(
        reopened
            .get_document_columns()
            .row_data(0)
            .map(|column| column.sort_order),
        Some(SortOrder::Ascending)
    );
    button(&reopened, "Dalej")?.invoke_accessible_default_action();
    assert_eq!(
        reopened.get_confirm_text(),
        "Sprzedaż · zlecenie 42 · Jan Kowalski · 250,00 zł\nDrukarka wystawi paragon fiskalny."
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn keyboard_navigation_selects_rows_and_enter_opens_the_selected_receipt_confirmation()
-> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    let template = app
        .borrow()
        .model
        .documents()
        .first()
        .ok_or("missing fixture")?
        .clone();
    app.borrow_mut().event(Event::Documents {
        items: (1..=30)
            .map(|number| Document {
                order_number: number,
                order_id: Uuid::from_u128(u128::try_from(number).unwrap_or_default()),
                ..template.clone()
            })
            .collect(),
        page: 1,
        has_previous: false,
        has_next: false,
    });
    app.borrow().sync();
    control(&window, "1", AccessibleRole::ListItem)?.invoke_accessible_default_action();
    for (key, row) in [(Key::DownArrow, 1), (Key::End, 29), (Key::Home, 0)] {
        window
            .window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        assert_eq!(
            window.get_selected(),
            row,
            "keyboard navigation changes the operator's selected row"
        );
    }
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::PageDown.into(),
    });
    assert!(window.get_selected() > 0);
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::PageUp.into(),
    });
    assert_eq!(window.get_selected(), 0);
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Return.into(),
    });
    assert_eq!(
        window.get_confirm_text(),
        "Sprzedaż · zlecenie 1 · Jan Kowalski · 250,00 zł\nDrukarka wystawi paragon fiskalny.",
        "Enter invokes the selected row's default action without printing yet"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn keys_behind_an_open_confirmation_cannot_change_what_it_submits() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    let template = app
        .borrow()
        .model
        .documents()
        .first()
        .ok_or("missing fixture")?
        .clone();
    app.borrow_mut().event(Event::Documents {
        items: (1..=3)
            .map(|number| Document {
                order_number: number,
                order_id: Uuid::from_u128(u128::try_from(number).unwrap_or_default()),
                ..template.clone()
            })
            .collect(),
        page: 1,
        has_previous: false,
        has_next: false,
    });
    app.borrow().sync();
    control(&window, "1", AccessibleRole::ListItem)?.invoke_accessible_default_action();
    for key in [Key::Return, Key::DownArrow, Key::DownArrow, Key::Return] {
        window
            .window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
    }
    assert!(window.get_confirming());
    assert_eq!(
        window.get_confirm_text(),
        "Sprzedaż · zlecenie 1 · Jan Kowalski · 250,00 zł\nDrukarka wystawi paragon fiskalny.",
        "the open confirmation keeps the receipt the operator opened"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn opt_out_is_persisted_before_the_printer_is_paired() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture_with_event_loop()?;
    window.set_tab(2);
    control(&window, "Udostępniaj diagnostykę", AccessibleRole::Checkbox)?
        .invoke_accessible_default_action();
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    let saved = Settings::load(&app.borrow().dir)?.ok_or("missing diagnostic preference")?;
    assert!(
        !saved.telemetry_enabled,
        "opting out must not require an API URL or token"
    );
    assert!(
        saved.api_url == Settings::default().api_url,
        "changing diagnostics must not invent connection settings"
    );
    assert_eq!(window.get_status(), "");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn unticking_diagnostics_stops_reporting_from_the_running_app() -> Result<(), Box<dyn Error>> {
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
    let (_temp, _app, window) = fixture()?;
    window.set_tab(2);
    let capture = Arc::new(Capture::default());
    let client = Arc::new(Client::from(
        telemetry::options()
            .dsn("https://public@example.invalid/1")
            .transport(Arc::clone(&capture)),
    ));
    let issues = || {
        capture
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .flat_map(Envelope::items)
            .filter(|item| matches!(item, EnvelopeItem::Event(_)))
            .count()
    };
    Hub::run(
        Arc::new(Hub::new(Some(Arc::clone(&client)), Arc::default())),
        || {
            let _id = sentry::capture_message("Worker operation failed", Level::Error);
            assert!(client.flush(Some(Duration::from_secs(1))));
            assert_eq!(issues(), 1, "diagnostics are shared before the opt-out");
            control(&window, "Udostępniaj diagnostykę", AccessibleRole::Checkbox)?
                .invoke_accessible_default_action();
            // The settings write is still pending: the opt-out must not wait for disk or restart.
            let _id = sentry::capture_message("Worker operation failed", Level::Error);
            assert!(client.flush(Some(Duration::from_secs(1))));
            assert_eq!(
                issues(),
                1,
                "nothing is reported after unticking the checkbox"
            );
            Ok::<(), Box<dyn Error>>(())
        },
    )
}

#[cfg(target_os = "macos")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Asserts fail-closed native startup after fallible UI setup"
)]
fn missing_native_quit_hook_blocks_worker_start() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    assert_eq!(window.get_status().as_str(), NATIVE_QUIT_ERROR);
    app.borrow_mut().spawn_worker();
    app.borrow().sync();
    assert!(
        app.borrow().worker.is_none(),
        "no fiscal worker may start without safe native quit"
    );
    assert_eq!(window.get_status().as_str(), NATIVE_QUIT_ERROR);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn status_publication_does_not_disable_settings_or_republish_on_acknowledgement()
-> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let simulator = Simulator::start();
    let app = Rc::new(RefCell::new(Application::new(
        Arc::clone(&dir),
        Settings::default(),
        simulator.link(),
        || Date::constant(2025, 10, 24),
    )));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&acknowledgements);
    app.borrow_mut().settings_writer = Some(preferences::Writer::spawn(dir, move |completion| {
        let _previous = observed.fetch_add(1, Ordering::SeqCst);
        let _scheduled = slint::invoke_from_event_loop(move || {
            with_app(move |app| {
                assert!(!app.model.saving, "status is not a settings save");
                let window = app.window.as_ref().ok_or("missing window")?;
                window.set_tab(2);
                assert_eq!(
                    button(window, "Uruchom")
                        .map_err(|error| error.to_string())?
                        .accessible_enabled(),
                    Some(true)
                );
                app.settings_saved(completion);
                assert!(app.writes_pending.is_empty());
                app.publish_status();
                assert!(
                    app.writes_pending.is_empty(),
                    "acknowledgement and unchanged state never form a save loop"
                );
                app.stop(true)
            });
        });
    })?);
    app.borrow_mut().launch()?;
    assert!(!app.borrow().model.saving);
    slint::run_event_loop_until_quit()?;
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn quit_exits_after_a_failed_status_write_and_reports_it() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    fs::create_dir(temporary.path().join("status.json"))?;
    let simulator = Simulator::start();
    let app = Rc::new(RefCell::new(Application::new(
        Arc::clone(&dir),
        Settings::default(),
        simulator.link(),
        || Date::constant(2025, 10, 24),
    )));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    let failures = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&failures);
    app.borrow_mut().settings_writer = Some(preferences::Writer::spawn(dir, move |completion| {
        if completion.is_err() {
            let _counted = counted.fetch_add(1, Ordering::SeqCst);
        }
        let _scheduled = slint::invoke_from_event_loop(move || {
            with_app(move |app| {
                app.settings_saved(completion);
                Ok(())
            });
        });
    })?);
    app.borrow_mut().launch()?;
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    assert!(
        failures.load(Ordering::SeqCst) >= 1,
        "the write really failed"
    );
    let state = app.borrow();
    assert!(
        state
            .model
            .message
            .starts_with("Nie można zapisać stanu aplikacji"),
        "the failure was shown: {}",
        state.model.message
    );
    drop(state);
    assert!(app.borrow().writes_pending.is_empty());
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Real report callbacks expose their exact confirmation parameters"
)]
fn report_confirmations_show_exact_kind_dates_and_summary_and_cancel_sends_nothing()
-> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    window.set_tab(1);
    for (label, expected) in [
        (
            "Drukuj raport dobowy",
            "Raport dobowy zamknie bieżącą sprzedaż dnia w pamięci fiskalnej. Wydrukować raport?",
        ),
        (
            "Drukuj raport miesięczny",
            "Wydrukować raport miesięczny za wrzesień 2025?",
        ),
        (
            "Drukuj raport okresowy",
            "Wydrukować raport szczegółowy za okres 29.02.2024 – 24.10.2025?",
        ),
    ] {
        control(&window, "Data początkowa", AccessibleRole::TextInput)?
            .set_accessible_value("29.02.2024");
        control(&window, "Data końcowa", AccessibleRole::TextInput)?
            .set_accessible_value("24.10.2025");
        button(&window, label)?.invoke_accessible_default_action();
        assert!(window.get_confirming());
        assert_eq!(window.get_confirm_text().as_str(), expected);
        button(&window, "Anuluj")?.invoke_accessible_default_action();
        assert!(!window.get_confirming());
        assert_eq!(button(&window, label)?.accessible_enabled(), Some(true));
    }
    control(&window, "Skrócony", AccessibleRole::Checkbox)?.invoke_accessible_default_action();
    button(&window, "Drukuj raport okresowy")?.invoke_accessible_default_action();
    assert_eq!(
        window.get_confirm_text(),
        "Wydrukować raport skrócony za okres 29.02.2024 – 24.10.2025?"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Invalid assistive row selections must clear selection"
)]
fn selecting_the_end_index_never_enables_a_nonexistent_receipt() -> Result<(), Box<dyn Error>> {
    let (_temporary, app, window) = fixture()?;
    ready(&app);
    window.invoke_selected_document(1);
    assert_eq!(window.get_selected(), -1);
    assert_eq!(button(&window, "Dalej")?.accessible_enabled(), Some(false));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A host without our icon cannot make hiding safe"
)]
fn close_with_a_host_but_without_an_application_icon_stays_visible() -> Result<(), Box<dyn Error>> {
    let (_temporary, app, window) = fixture()?;
    app.borrow_mut().close_after_probe(true);
    app.borrow().sync();
    assert!(window.window().is_visible());
    assert!(app.borrow().window.is_some());
    assert_eq!(
        window.get_status(),
        "Zasobnik systemowy jest niedostępny. Aby zakończyć aplikację, kliknij Zakończ."
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Blocked telemetry saves must not conceal fiscal evidence"
)]
fn diagnostics_completion_preserves_the_unresolved_operation_notice() -> Result<(), Box<dyn Error>>
{
    let (_temporary, app, window) = fixture_with_event_loop()?;
    app.borrow_mut().event(Event::Device {
        status: None,
        blocked: true,
        report_unresolved: true,
        orphaned_receipt: None,
    });
    app.borrow_mut()
        .model
        .problem("Nierozstrzygnięty raport".into());
    app.borrow().sync();
    window.set_tab(2);
    control(&window, "Udostępniaj diagnostykę", AccessibleRole::Checkbox)?
        .invoke_accessible_default_action();
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    assert_eq!(window.get_status(), "Nierozstrzygnięty raport");
    Ok(())
}

// Startup creates the tray, and Slint's AppKit tray requires the main thread.
#[cfg(not(target_os = "macos"))]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The actual top-level startup must construct and stop the app"
)]
fn production_run_opens_the_window_and_publishes_stopped_state() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let simulator = Simulator::start();
    slint::invoke_from_event_loop(|| {
        with_app(|app| {
            assert!(
                app.window
                    .as_ref()
                    .is_some_and(|window| window.window().is_visible())
            );
            app.stop(true)
        });
    })?;
    run(Arc::clone(&dir), Settings::default(), simulator.link())?;
    let status = Snapshot::load_at(dir.path())?.ok_or("missing stopped publication")?;
    assert!(!status.processing && !status.connected);
    assert!(
        simulator.frames().is_empty(),
        "startup never opens a printer"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A native close request must keep an iconless app reachable"
)]
fn native_close_request_keeps_the_window_until_the_tray_probe_finishes()
-> Result<(), Box<dyn Error>> {
    let (_temporary, _app, window) = fixture()?;
    window.window().dispatch_event(WindowEvent::CloseRequested);
    assert!(window.window().is_visible());
    Ok(())
}

fn receipt_view(number: u32, state: &str) -> Result<JobView, serde_json::Error> {
    serde_json::from_value(receipt_view_json(number, state))
}

fn receipt_view_json(number: u32, state: &str) -> serde_json::Value {
    use agent::testing::{job_json, with};
    serde_json::json!({
        "job": with(job_json(), &serde_json::json!({
            "id": Uuid::from_u128(u128::from(number)),
            "orderNumber": number, "state": state, "issuedAt": "2025-10-24T10:00:00Z",
            "advances": [{"vatLetter": "A", "vatRate": "23", "amount": "50.00"}]
        })),
        "customerName": "Łukasz Kowalski", "vehiclePlate": "WA 12345", "source": "staff"
    })
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert server history through accessible controls"
)]
fn receipts_show_server_fields_details_and_all_seven_states() -> Result<(), Box<dyn Error>> {
    let (_temporary, app, window) = fixture()?;
    for (state, status) in [
        ("queued", "W kolejce"),
        ("taken", "Drukuje się…"),
        ("done", "Wydrukowano"),
        ("failed", "Nie wydrukowano"),
        ("expired", "Wygasło"),
        ("unknown", "Do sprawdzenia"),
        ("voided", "Anulowano"),
    ] {
        let mut view = receipt_view(1042, state)?;
        view.job.receipt_number = Some("2187".into());
        view.job.result = Some("Nieprawidłowa stawka VAT".into());
        app.borrow_mut().event(Event::Jobs {
            items: vec![view],
            page: 1,
            has_previous: false,
            has_next: false,
        });
        app.borrow().sync();
        let row = control(&window, "Zlecenie 1042", AccessibleRole::ListItem)?;
        let description = row.accessible_description().ok_or("missing description")?;
        for field in [
            status,
            "Łukasz Kowalski · WA 12345",
            "250,00 zł",
            "Nr 2187 · 24.10.2025",
            "Nieprawidłowa stawka VAT",
        ] {
            assert!(description.contains(field), "{description} lacks {field}");
        }
        row.invoke_accessible_default_action();
        let detail = window.get_receipt_detail();
        assert_eq!(
            (detail.order.as_str(), detail.customer.as_str()),
            ("Zlecenie 1042", "Łukasz Kowalski · WA 12345")
        );
        let line = detail.lines.row_data(0).ok_or("missing line")?;
        assert_eq!(
            [line.name, line.quantity, line.price, line.total].map(|text| text.to_string()),
            ["Wymiana oleju", "2", "100,00 zł", "200,00 zł"]
        );
        assert_eq!(
            detail
                .totals
                .iter()
                .map(|total| format!("{} {}", total.label, total.amount))
                .collect::<Vec<_>>(),
            [
                "Razem 250,00 zł",
                "Odliczona zaliczka · VAT 23% 50,00 zł",
                "Płatność: Karta płatnicza 250,00 zł"
            ]
        );
        assert_eq!(detail.warning.is_empty(), state != "unknown");
        assert_eq!(window.get_receipt_selected(), 0);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert the rendered receipt through accessible controls"
)]
fn a_receipt_shows_only_fields_with_values_and_a_single_page_hides_paging()
-> Result<(), Box<dyn Error>> {
    let (_temporary, app, window) = fixture()?;
    let mut view = receipt_view(1042, "queued")?;
    view.customer_name = None;
    view.vehicle_plate = None;
    view.job.issued_at = None;
    app.borrow_mut().event(Event::Jobs {
        items: vec![view],
        page: 1,
        has_previous: false,
        has_next: false,
    });
    app.borrow().sync();
    let row = window.get_receipts().row_data(0).ok_or("missing row")?;
    assert_eq!(
        [row.customer, row.amount, row.receipt, row.note].map(|text| text.to_string()),
        ["", "250,00 zł", "", ""],
        "nothing describes missing data"
    );
    let detail = window.get_receipt_detail();
    assert_eq!(
        [detail.customer, detail.buyer_nip, detail.warning].map(|text| text.to_string()),
        ["", "", ""]
    );
    for label in [
        "Historia od początku",
        "Poprzednia strona historii",
        "Następna strona historii",
    ] {
        assert!(
            ElementHandle::find_by_accessible_label(&window, label)
                .next()
                .is_none(),
            "{label} is hidden on the only page"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Keyboard events drive the real focused controls"
)]
fn receipt_list_keyboard_navigation_and_search_shortcuts() -> Result<(), Box<dyn Error>> {
    let (_temporary, app, window) = fixture()?;
    let items = (1..=30)
        .map(|number| receipt_view(number, "queued"))
        .collect::<Result<Vec<_>, _>>()?;
    app.borrow_mut().event(Event::Jobs {
        items,
        page: 1,
        has_previous: false,
        has_next: false,
    });
    app.borrow().sync();
    control(&window, "Zlecenie 1", AccessibleRole::ListItem)?.invoke_accessible_default_action();
    for (key, selected) in [(Key::DownArrow, 1), (Key::End, 29), (Key::Home, 0)] {
        window
            .window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        assert_eq!(window.get_receipt_selected(), selected);
    }
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::PageDown.into(),
    });
    let page = window.get_receipt_selected();
    assert!(page > 0 && page < 29);
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::PageUp.into(),
    });
    assert_eq!(window.get_receipt_selected(), 0);
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    window
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: "f".into() });
    window
        .window()
        .dispatch_event(WindowEvent::KeyReleased { text: "f".into() });
    window.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    window
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: "2".into() });
    assert_eq!(
        window.get_receipt_search(),
        "2",
        "Ctrl+F focuses server search"
    );
    assert_eq!(
        window.get_receipts().row_count(),
        30,
        "typing never filters locally"
    );
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Escape.into(),
    });
    assert_eq!(window.get_receipt_search(), "");
    assert_eq!(window.get_receipts().row_count(), 30);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Server ordering and selection are operator-visible state"
)]
fn receipt_refresh_preserves_server_order_and_selected_job_identity() -> Result<(), Box<dyn Error>>
{
    let (_temporary, app, window) = fixture()?;
    app.borrow_mut().event(Event::Jobs {
        items: vec![
            receipt_view(3, "done")?,
            receipt_view(1, "done")?,
            receipt_view(2, "done")?,
        ],
        page: 1,
        has_previous: false,
        has_next: true,
    });
    app.borrow().sync();
    assert_eq!(
        window
            .get_receipts()
            .iter()
            .map(|row| row.order.to_string())
            .collect::<Vec<_>>(),
        ["Zlecenie 3", "Zlecenie 1", "Zlecenie 2"]
    );
    control(&window, "Zlecenie 2", AccessibleRole::ListItem)?.invoke_accessible_default_action();
    app.borrow_mut().event(Event::Jobs {
        items: vec![receipt_view(4, "done")?, receipt_view(2, "done")?],
        page: 1,
        has_previous: false,
        has_next: false,
    });
    app.borrow().sync();
    assert_eq!(window.get_receipt_selected(), 1);
    assert_eq!(window.get_receipt_detail().order, "Zlecenie 2");
    assert!(
        ElementHandle::find_by_accessible_label(&window, "Wyjaśnij paragon")
            .next()
            .is_none()
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The header shows connection and printer state only while something is wrong"
)]
fn header_shows_holuto_and_printer_only_while_something_is_wrong() -> Result<(), Box<dyn Error>> {
    let (_temporary, app, window) = fixture()?;
    assert_eq!(
        control(&window, "Holuto", AccessibleRole::Text)?
            .accessible_value()
            .as_deref(),
        Some("Nie połączono")
    );
    app.borrow_mut().model.running = true;
    app.borrow().sync();
    assert_eq!(
        control(&window, "Holuto", AccessibleRole::Text)?
            .accessible_value()
            .as_deref(),
        Some("Łączenie…")
    );
    ready(&app);
    for label in ["Holuto", "Drukarka"] {
        assert!(
            ElementHandle::find_by_accessible_label(&window, label)
                .next()
                .is_none(),
            "a working {label} is not announced"
        );
    }
    app.borrow_mut()
        .model
        .device
        .as_mut()
        .ok_or("missing ready printer")?
        .problems
        .push("Brak papieru".into());
    app.borrow().sync();
    assert_eq!(
        control(&window, "Drukarka", AccessibleRole::Text)?
            .accessible_value()
            .as_deref(),
        Some("Niegotowa: Brak papieru")
    );
    app.borrow_mut().event(Event::Device {
        status: None,
        blocked: true,
        report_unresolved: false,
        orphaned_receipt: None,
    });
    app.borrow().sync();
    assert!(
        ElementHandle::find_by_accessible_label(&window, "Holuto")
            .next()
            .is_none()
    );
    assert_eq!(
        control(&window, "Drukarka", AccessibleRole::Text)?
            .accessible_value()
            .as_deref(),
        Some("Zablokowana: wymaga rozstrzygnięcia")
    );
    Ok(())
}
