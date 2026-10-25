#[cfg(target_os = "linux")]
#[path = "tests/autostart.rs"]
mod autostart;
#[path = "tests/end_to_end.rs"]
mod end_to_end;
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

use agent::documents::{Document, ReceiptKind};
use fiscal::device::{Device, Vendor};
use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use jiff::Timestamp;
use posnet::{capabilities::capabilities, simulator::Simulator};
use sentry::{Client, ClientOptions, Envelope, Hub, Level, Transport, protocol::EnvelopeItem};
use slint::{
    Model as _,
    platform::{Key, WindowEvent},
};
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
    let app = Rc::new(RefCell::new(Application::new(
        dir,
        settings,
        link,
        Date::constant(2025, 10, 24),
    )));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    app.borrow_mut().launch()?;
    Ok(app)
}

fn control(
    window: &AppWindow,
    label: &str,
    role: AccessibleRole,
) -> Result<ElementHandle, Box<dyn Error>> {
    ElementHandle::find_by_accessible_label(window, label)
        .find(|element| element.accessible_role() == Some(role))
        .ok_or_else(|| format!("missing accessible control {label}").into())
}
fn button(window: &AppWindow, label: &str) -> Result<ElementHandle, Box<dyn Error>> {
    control(window, label, AccessibleRole::Button)
}
fn ready(app: &Rc<RefCell<Application>>) {
    let mut app = app.borrow_mut();
    app.event(Event::Connected {
        printer_id: "printer-42".into(),
        unique_number: "TST0000000042".into(),
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
        has_next: true,
    });
    app.model.selected = Some(0);
    app.sync();
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn queue_confirmation_shows_amount_nip_and_local_pending_state() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    control(&window, "NIP nabywcy", AccessibleRole::TextInput)?.set_accessible_value("1234567890");
    button(&window, "Dodaj do wydruku")?.invoke_accessible_default_action();
    assert_eq!(
        window.get_confirm_text().as_str(),
        concat!(
            "Sprzedaż · zlecenie 42 · 250,00 zł\nNIP: 1234567890\n",
            "Zatwierdzenie wystawi paragon fiskalny."
        ),
        "operator sees the binding amount and NIP"
    );
    let confirm = ElementHandle::find_by_accessible_label(&window, "Dodaj do wydruku")
        .filter(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .last()
        .ok_or("missing confirmation button")?;
    confirm.invoke_accessible_default_action();
    assert_eq!(
        button(&window, "Dodaj do wydruku")?.accessible_enabled(),
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
        button(&window, "Dodaj do wydruku")?.accessible_enabled(),
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
    assert_eq!(
        button(&window, "Poprzednia strona")?.accessible_enabled(),
        Some(false),
        "first page has no predecessor"
    );
    assert_eq!(
        button(&window, "Następna strona")?.accessible_enabled(),
        Some(true),
        "cursor exposes next page"
    );
    app.borrow_mut().event(Event::Documents {
        items: Vec::new(),
        page: 2,
        has_next: false,
    });
    app.borrow().sync();
    assert_eq!(
        window.get_page_label().as_str(),
        "Strona 2",
        "operator sees current page"
    );
    assert_eq!(
        button(&window, "Poprzednia strona")?.accessible_enabled(),
        Some(true),
        "second page can go back"
    );
    assert_eq!(
        button(&window, "Następna strona")?.accessible_enabled(),
        Some(false),
        "last page has no successor"
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
    // Crash fixtures: a periodic report whose command may have reached the printer, in the
    // current format and with the legacy key only the worker may migrate.
    for identity_key in ["device_unique_number", "device_serial"] {
        let temp = TempDir::new()?;
        let dir = Arc::new(StateDir::open(temp.path().to_owned())?);
        let interrupted = format!(
            r#"{{
  "report": {{
    "report": {{"kind": "periodic", "from": "2026-09-01", "to": "2026-09-30", "summary": false}},
    "vendor": "posnet",
    "{identity_key}": "POS0000123456",
    "started_at": "2026-10-16T10:00:00Z",
    "device": {{"dispatching": {{"stage": "periodicrepbydates"}}}},
    "acknowledged_at": null
  }}
}}"#
        );
        fs::write(temp.path().join("operation.json"), &interrupted)?;
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
            "`workshop status` must not report an unresolved report as unblocked ({identity_key})"
        );
        assert_eq!(
            fs::read_to_string(temp.path().join("operation.json"))?,
            interrupted,
            "launch never rewrites the fiscal journal ({identity_key})"
        );
        // Save feedback stays hidden while blocked, so launch must explain the block itself.
        let explanation = (
            "Poprzednia operacja jest nierozstrzygnięta. Uruchom obsługę drukarki, aby ją \
             wyjaśnić."
                .to_owned(),
            Tone::Error,
        );
        assert_eq!(
            (app.borrow().model.message.clone(), app.borrow().model.tone),
            explanation,
            "launch explains the block instead of the pairing prompt ({identity_key})"
        );
        app.borrow_mut()
            .settings_saved(Ok(preferences::Outcome::Saved));
        assert_eq!(
            (app.borrow().model.message.clone(), app.borrow().model.tone),
            explanation,
            "a later save keeps the block explanation visible ({identity_key})"
        );
        APPLICATION.with(|slot| *slot.borrow_mut() = None);
    }
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
        has_next: true,
    });
    state.event(Event::Stopped);
    // A new worker's pager starts at the first page.
    state.event(Event::Connected {
        printer_id: "printer-42".into(),
        unique_number: "TST0000000042".into(),
        port: "test".into(),
    });
    state.sync();
    drop(state);
    assert_eq!(window.get_page_label().as_str(), "Strona 1");
    assert_eq!(
        button(&window, "Poprzednia strona")?.accessible_enabled(),
        Some(false),
        "the new session has no previous page"
    );
    assert!(
        app.borrow().model.documents().is_empty(),
        "documents from the stopped session are not offered for printing"
    );
    assert_eq!(
        button(&window, "Dodaj do wydruku")?.accessible_enabled(),
        Some(false),
        "no stale selection can be queued"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn stopped_worker_replaces_progress_text_but_keeps_errors() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    let mut state = app.borrow_mut();
    // State left by `stop` while the worker finishes its operation.
    state.model.running = true;
    state.model.stopping = true;
    state.model.say(
        "Zatrzymywanie po zakończeniu bieżącej operacji…".into(),
        Tone::Info,
    );
    state.event(Event::Stopped);
    state.sync();
    assert_eq!(
        window.get_status().as_str(),
        "Zatrzymano.",
        "a requested stop reports completion"
    );
    state.model.running = true;
    state.event(Event::Status {
        text: "Błąd: brak połączenia".into(),
        tone: Tone::Error,
    });
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
    Ok((temp, app, window))
}

fn save_and_quit(
    app: &Rc<RefCell<Application>>,
    window: &AppWindow,
) -> Result<Settings, Box<dyn Error>> {
    button(window, "Zapisz")?.invoke_accessible_default_action();
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    Ok(Settings::load(&app.borrow().dir)?.ok_or("missing saved settings")?)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn save_persists_operator_connection_choices_before_quitting() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = paired_fixture()?;
    control(&window, "Adres API", AccessibleRole::TextInput)?
        .set_accessible_value("https://api.example.test");
    let saved = save_and_quit(&app, &window)?;
    assert_eq!(
        saved.api_url, "https://api.example.test",
        "save captures API input"
    );
    assert_eq!(
        saved.token.as_str(),
        "",
        "a token paired with another API address is never saved for the new one"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn a_reverted_api_address_edit_keeps_the_saved_pairing() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = paired_fixture()?;
    let address = control(&window, "Adres API", AccessibleRole::TextInput)?;
    address.set_accessible_value("https://paired.example.tes");
    // Any UI callback captures the half-edited field.
    window.invoke_sort_documents(0, false);
    address.set_accessible_value("https://paired.example.test");
    let saved = save_and_quit(&app, &window)?;
    assert_eq!(saved.api_url, "https://paired.example.test");
    assert_eq!(
        saved.token.as_str(),
        "already-paired-secret",
        "the pairing belongs to the saved address the operator restored"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn a_status_write_acknowledgement_keeps_settings_typed_meanwhile() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture_with_event_loop()?;
    control(&window, "Adres API", AccessibleRole::TextInput)?
        .set_accessible_value("https://typed.example.test");
    control(&window, "Kod parowania", AccessibleRole::TextInput)?.set_accessible_value("ABCD-1234");
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    assert_eq!(
        (
            window.get_api_url().as_str(),
            window.get_pairing_code().as_str()
        ),
        ("https://typed.example.test", "ABCD-1234"),
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
        Date::constant(2025, 10, 24),
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

/// Returns false if the deadline forced the event loop to quit.
fn quit_before_deadline() -> Result<bool, Box<dyn Error>> {
    let (finished, quit) = mpsc::channel::<()>();
    let deadline = thread::Builder::new()
        .name("quit deadline".into())
        .spawn(move || {
            // Bound a failed test instead of hanging the suite.
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
            app.model
                .say("Nested callback completed".into(), Tone::Success);
            Ok(())
        });
        app.model.say("Outer callback completed".into(), Tone::Info);
        Ok(())
    });
    assert_eq!(
        window.get_status().as_str(),
        "Nested callback completed",
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
    state.event(Event::Status {
        text: "Błąd: Holuto niedostępne".into(),
        tone: Tone::Error,
    });
    state.sync();
    drop(state);
    button(&window, "Wyjaśnij paragon")?.invoke_accessible_default_action();
    assert!(
        window
            .get_confirm_text()
            .contains("Paragon 17 został wydrukowany dla zlecenia 42"),
        "acknowledgement identifies the printed receipt"
    );
    assert!(
        window
            .get_confirm_text()
            .contains("Nie wydrukuje paragonu ponownie"),
        "confirmation explains no reprint"
    );
    button(&window, "Anuluj")?.invoke_accessible_default_action();
    // A submitted acknowledgement would mark a command in flight and hide the button.
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
    button(&window, "Dodaj do wydruku")?.invoke_accessible_default_action();
    assert!(
        window.get_confirm_text().contains("zlecenie 42"),
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

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn close_with_a_live_tray_hides_and_reopening_restores_the_window() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
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
    assert!(
        app.borrow()
            .window
            .as_ref()
            .is_some_and(|window| window.window().is_visible()),
        "tray open recreates a visible window"
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
        has_next: false,
    });
    app.borrow().sync();
    control(&window, "1", AccessibleRole::ListItem)?.invoke_accessible_default_action();
    for (key, row) in [
        (Key::DownArrow, 1),
        (Key::End, 29),
        (Key::Home, 0),
        (Key::PageDown, 7),
        (Key::PageUp, 0),
    ] {
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
        text: Key::Return.into(),
    });
    assert!(
        window.get_confirm_text().contains("zlecenie 1"),
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
        "Sprzedaż · zlecenie 1 · 250,00 zł\nNIP: —\nZatwierdzenie wystawi paragon fiskalny.",
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
        saved.api_url.is_empty(),
        "changing diagnostics must not invent connection settings"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Asserts fail-closed native startup after fallible UI setup"
)]
fn missing_native_quit_hook_blocks_worker_start() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    app.borrow_mut().spawn_worker();
    app.borrow().sync();
    assert!(
        app.borrow().worker.is_none(),
        "no fiscal worker may start without safe native quit"
    );
    assert!(
        window
            .get_status()
            .contains("Obsługa drukarki jest wyłączona"),
        "operator sees why starting is refused"
    );
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
        Date::constant(2025, 10, 24),
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
                assert!(app.status_settled.is_some());
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
    // A directory in place of status.json makes every status write fail.
    fs::create_dir(temporary.path().join("status.json"))?;
    let simulator = Simulator::start();
    let app = Rc::new(RefCell::new(Application::new(
        Arc::clone(&dir),
        Settings::default(),
        simulator.link(),
        Date::constant(2025, 10, 24),
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
    assert_eq!(
        app.borrow().model.tone,
        Tone::Error,
        "the failure was shown"
    );
    assert!(app.borrow().writes_pending.is_empty());
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}
