#[cfg(target_os = "linux")]
#[path = "tests/autostart.rs"]
mod autostart;
#[path = "tests/end_to_end.rs"]
mod end_to_end;
#[path = "tests/pairing.rs"]
mod pairing;
#[cfg(not(target_os = "macos"))]
#[path = "tests/start.rs"]
mod start;

use std::{
    error::Error,
    fs,
    sync::{
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
        orphaned_receipt: false,
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

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn save_persists_operator_connection_choices_before_quitting() -> Result<(), Box<dyn Error>> {
    let (_temp, app, window) = fixture_with_event_loop()?;
    app.borrow_mut().model.settings.api_url = "https://paired.example.test".into();
    app.borrow_mut().model.settings.token = "already-paired-secret".into();
    app.borrow().sync();
    control(&window, "Adres API", AccessibleRole::TextInput)?
        .set_accessible_value("https://api.example.test");
    button(&window, "Zapisz")?.invoke_accessible_default_action();
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    let saved = Settings::load(&app.borrow().dir)?.ok_or("missing saved settings")?;
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
                orphaned_receipt: false,
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
    // Launch has submitted its status; the event loop delivers that acknowledgement
    // only after the newer status below could not be submitted.
    let (_temp, app, _window) = fixture_with_event_loop()?;
    app.borrow_mut().settings_writer = Some(preferences::Writer::closed());
    app.borrow_mut().event(Event::Device {
        status: None,
        blocked: true,
        report_unresolved: false,
        orphaned_receipt: false,
    });
    app.borrow_mut().stop(true)?;
    assert!(
        quit_before_deadline()?,
        "the stale launch acknowledgement must not keep quit waiting"
    );
    Ok(())
}

/// Runs the event loop; `false` if quitting had to be forced after the deadline.
fn quit_before_deadline() -> Result<bool, Box<dyn Error>> {
    let (finished, quit) = mpsc::channel::<()>();
    let deadline = thread::Builder::new()
        .name("quit deadline".into())
        .spawn(move || {
            // Failure diagnostic only: end a hung loop instead of hanging the suite.
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
    state.model.blocked = true;
    state.model.orphaned_receipt = true;
    state
        .model
        .say("Zlecenie 42 · paragon 17".into(), Tone::Error);
    state.sync();
    drop(state);
    button(&window, "Wyjaśnij paragon")?.invoke_accessible_default_action();
    assert!(
        window
            .get_confirm_text()
            .contains("Zlecenie 42 · paragon 17"),
        "acknowledgement identifies the printed receipt"
    );
    assert!(
        window
            .get_confirm_text()
            .contains("Nie wydrukuje paragonu ponownie"),
        "confirmation explains no reprint"
    );
    button(&window, "Anuluj")?.invoke_accessible_default_action();
    assert!(
        app.borrow().model.blocked,
        "cancelling keeps the fiscal block"
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
    // Returns only because finish_quit accepted the failed (settled) stopped snapshot.
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
