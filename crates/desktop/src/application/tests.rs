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

use agent::settings::Theme;
use fiscal::device::{Device, Vendor};
use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use jiff::Timestamp;
use posnet::{capabilities::capabilities, simulator::Simulator};
use sentry::{Client, ClientOptions, Envelope, Hub, Level, Transport, protocol::EnvelopeItem};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use tempfile::TempDir;

use super::*;

type UiFixture = (TempDir, Rc<RefCell<Application>>, AppWindow);

// Compare what the operator reads; Fluent's bidi isolation is verified in i18n tests.
fn visible_text(text: &str) -> String {
    text.chars()
        .filter(|character| !matches!(character, '\u{2068}' | '\u{2069}'))
        .collect()
}

fn polish_settings() -> Settings {
    Settings {
        language: i18n::LanguagePreference::Pl,
        ..Settings::default()
    }
}

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
        polish_settings(),
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
    app.sync();
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert accessible application behavior"
)]
fn status_is_the_first_tab_and_names_each_read_only_value() -> Result<(), Box<dyn Error>> {
    let (_temporary, app, window) = fixture()?;
    let shown = |window: &AppWindow| -> Result<Vec<String>, Box<dyn Error>> {
        ["Obsługa drukarki", "Holuto", "Drukarka", "Stan drukarki"]
            .into_iter()
            .map(|label| {
                // The header repeats Holuto and Drukarka while something is wrong; Stan is last.
                ElementHandle::find_by_accessible_label(window, label)
                    .filter(|element| element.accessible_role() == Some(AccessibleRole::Text))
                    .last()
                    .and_then(|element| element.accessible_value())
                    .map(|value| value.to_string())
                    .ok_or_else(|| format!("missing Stan value {label}").into())
            })
            .collect()
    };
    let start_hint = |window: &AppWindow| {
        ElementHandle::find_by_accessible_label(window, "Uruchom obsługę drukarki w Ustawieniach.")
            .next()
            .is_some()
    };
    assert_eq!(window.get_tab(), 0);
    assert_eq!(
        shown(&window)?,
        [
            "Zatrzymana",
            "Nie połączono",
            "—",
            "Niegotowa: brak połączenia"
        ]
    );
    assert!(
        ElementHandle::find_by_accessible_label(
            &window,
            "Paragony wystawiasz i przeglądasz w Holuto. Zadania z kolejki Holuto drukują się \
             tutaj automatycznie."
        )
        .next()
        .is_some(),
        "Stan sends the operator to Holuto for receipts"
    );
    assert!(start_hint(&window), "a stopped app asks to start");
    window.set_tab(2);
    app.borrow_mut().capture();
    app.borrow_mut().model.running = true;
    ready(&app);
    assert_eq!(window.get_tab(), 0, "connecting shows Stan");
    assert_eq!(
        shown(&window)?,
        [
            "Uruchomiona",
            "Połączono",
            "Posnet · TST0000000042 · test",
            "Gotowa"
        ]
    );
    assert!(!start_hint(&window), "a running app does not ask to start");
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
    let app = make_app(dir, polish_settings(), simulator.link())?;
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
            app.borrow().model.message.to_string(),
            explanation,
            "launch explains the block instead of the pairing prompt"
        );
        let settings = app.borrow().model.settings.clone();
        app.borrow_mut()
            .settings_saved(Ok(preferences::Outcome::Saved(settings)));
        assert_eq!(
            app.borrow().model.message.to_string(),
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
            Err(LinkError::Invalid(
                format!("unexpected open of {port}").into(),
            ))
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
            ..polish_settings()
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
        ..polish_settings()
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
        polish_settings(),
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
            .to_string()
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
        visible_text(window.get_confirm_text().as_str()),
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
fn close_without_a_tray_host_keeps_the_window_visible() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    app.borrow_mut().close_after_probe(false);
    assert!(
        window.window().is_visible(),
        "closing without a tray must not strand the operator"
    );
    assert_eq!(
        app.borrow().model.message.to_string(),
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
    let (_temp, app, window) = fixture()?;
    ready(&app);
    window.set_tab(1);
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
    assert_eq!(reopened.get_tab(), 1, "the operator's tab survives");
    assert_eq!(
        reopened.get_printer_identity(),
        "Posnet · TST0000000042 · test"
    );
    button(&reopened, "Drukuj raport dobowy")?.invoke_accessible_default_action();
    assert_eq!(reopened.get_confirm_title(), "Raport dobowy");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn controls_behind_an_open_confirmation_cannot_change_its_report() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    window.set_tab(1);
    button(&window, "Drukuj raport dobowy")?.invoke_accessible_default_action();
    for behind in ["Drukuj raport miesięczny", "Drukuj raport okresowy"] {
        button(&window, behind)?.invoke_accessible_default_action();
    }
    assert!(window.get_confirming());
    assert_eq!(
        (window.get_confirm_title(), window.get_confirm_text()),
        (
            "Raport dobowy".into(),
            "Raport dobowy zamknie bieżącą sprzedaż dnia w pamięci fiskalnej. Wydrukować raport?"
                .into()
        ),
        "the open confirmation keeps the report the operator opened"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn enter_in_an_open_confirmation_cancels_without_printing() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture()?;
    ready(&app);
    window.set_tab(1);
    let enter = || {
        let key = SharedString::from(Key::Return);
        window
            .window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
        window
            .window()
            .dispatch_event(WindowEvent::KeyReleased { text: key });
    };
    // Reopening builds a new dialog, so the safe focus must be set every time.
    for _ in 0..2 {
        button(&window, "Drukuj raport dobowy")?.invoke_accessible_default_action();
        assert!(window.get_confirming());
        enter();
        assert!(!window.get_confirming(), "Enter closes the dialog");
        assert_eq!(
            button(&window, "Drukuj raport dobowy")?.accessible_enabled(),
            Some(true),
            "nothing was submitted: an in-flight report disables it"
        );
    }
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
fn theme_defaults_to_system_and_a_chosen_theme_is_saved_before_pairing()
-> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture_with_event_loop()?;
    window.set_tab(2);
    let theme = control(&window, "Motyw", AccessibleRole::Combobox)?;
    assert_eq!(theme.accessible_value().as_deref(), Some("Systemowy"));
    theme.invoke_accessible_expand_action();
    let dark = ElementHandle::find_by_accessible_label(&window, "Ciemny")
        .find(|element| element.accessible_role() == Some(AccessibleRole::ListItem))
        .ok_or("missing Ciemny choice")?;
    dark.mock_single_click(PointerEventButton::Left);
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    let saved = Settings::load(&app.borrow().dir)?.ok_or("missing theme preference")?;
    assert_eq!(saved.theme, Theme::Dark);
    assert_eq!(
        saved.api_url,
        Settings::default().api_url,
        "a look change must not invent connection settings"
    );
    assert_eq!(theme.accessible_value().as_deref(), Some("Ciemny"));
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
        polish_settings(),
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
        polish_settings(),
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
            .to_string()
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
        assert_eq!(visible_text(window.get_confirm_text().as_str()), expected);
        button(&window, "Anuluj")?.invoke_accessible_default_action();
        assert!(!window.get_confirming());
        assert_eq!(button(&window, label)?.accessible_enabled(), Some(true));
    }
    control(&window, "Skrócony", AccessibleRole::Checkbox)?.invoke_accessible_default_action();
    button(&window, "Drukuj raport okresowy")?.invoke_accessible_default_action();
    assert_eq!(
        visible_text(window.get_confirm_text().as_str()),
        "Wydrukować raport skrócony za okres 29.02.2024 – 24.10.2025?"
    );
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
    run(Arc::clone(&dir), polish_settings(), simulator.link())?;
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

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The header shows connection and printer state only while something is wrong"
)]
fn header_shows_holuto_and_printer_only_while_something_is_wrong() -> Result<(), Box<dyn Error>> {
    let (_temporary, app, window) = fixture()?;
    // Stan always lists Holuto and Drukarka after the header's copies.
    let values = |label| {
        ElementHandle::find_by_accessible_label(&window, label)
            .filter(|element| element.accessible_role() == Some(AccessibleRole::Text))
            .map(|element| visible_text(element.accessible_value().unwrap_or_default().as_str()))
            .collect::<Vec<_>>()
    };
    assert_eq!(values("Holuto"), ["Nie połączono", "Nie połączono"]);
    app.borrow_mut().model.running = true;
    app.borrow().sync();
    assert_eq!(values("Holuto"), ["Łączenie…", "Łączenie…"]);
    ready(&app);
    assert_eq!(
        (values("Holuto"), values("Drukarka")),
        (
            vec!["Połączono".to_owned()],
            vec!["Posnet · TST0000000042 · test".to_owned()]
        ),
        "a working setup is announced only in Stan"
    );
    app.borrow_mut()
        .model
        .device
        .as_mut()
        .ok_or("missing ready printer")?
        .problems
        .push("Brak papieru".into());
    app.borrow().sync();
    assert_eq!(
        values("Drukarka"),
        ["Niegotowa: Brak papieru", "Posnet · TST0000000042 · test"]
    );
    app.borrow_mut().event(Event::Device {
        status: None,
        blocked: true,
        report_unresolved: false,
        orphaned_receipt: None,
    });
    app.borrow().sync();
    assert_eq!(values("Holuto"), ["Połączono"]);
    assert_eq!(
        values("Drukarka"),
        [
            "Zablokowana: wymaga rozstrzygnięcia",
            "Posnet · TST0000000042 · test"
        ]
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Language selection must persist without losing operator input"
)]
fn language_selection_translates_the_open_window_and_persists_before_pairing()
-> Result<(), Box<dyn Error>> {
    let (_temporary, app, window) = fixture_with_event_loop()?;
    app.borrow_mut().event(Event::Problem(Some(
        Message::new("fiscal-timeout").arg("command", "trinit"),
    )));
    app.borrow().sync();
    window.set_tab(2);
    control(&window, "Kod parowania", AccessibleRole::TextInput)?.set_accessible_value("ABCD-1234");
    let language = control(&window, "Język", AccessibleRole::Combobox)?;
    language.invoke_accessible_expand_action();
    ElementHandle::find_by_accessible_label(&window, "Español")
        .find(|element| element.accessible_role() == Some(AccessibleRole::ListItem))
        .ok_or("missing Spanish language choice")?
        .mock_single_click(PointerEventButton::Left);
    assert_eq!(button(&window, "Iniciar")?.accessible_enabled(), Some(true));
    assert!(window.get_status().starts_with("Sin respuesta a "));
    assert!(window.get_status().contains("trinit"));
    assert!(
        window
            .get_status()
            .ends_with("No se ha reenviado el comando.")
    );
    assert_eq!(
        control(&window, "Código de vinculación", AccessibleRole::TextInput)?
            .accessible_value()
            .as_deref(),
        Some("ABCD-1234")
    );
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    let saved = fs::read_to_string(app.borrow().dir.path().join("settings.json"))?;
    let saved: serde_json::Value = serde_json::from_str(&saved)?;
    assert_eq!(saved["language"], "es");
    assert!(!app.borrow().model.running);
    Ok(())
}
