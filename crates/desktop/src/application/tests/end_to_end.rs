use std::sync::atomic::AtomicBool;

#[cfg(target_os = "macos")]
use super::super::macos;
#[cfg(target_os = "macos")]
use objc2::{runtime::NSObject, sel};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSApplicationTerminateReply;

use agent::testing::{self, Holuto, ORDER_ID};
use serde_json::json;
use slint::Model as _;

use super::*;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "End-to-end tests assert while propagating fixture errors"
)]
fn operator_queue_prints_and_shows_accepted_receipt_history() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Arc::new(Simulator::start());
    simulator.set_clock("2025-10-24T10:00:00Z".parse()?);
    let holuto = Arc::new(Holuto::new());
    holuto.documents(vec![json!({
        "kind": "sale", "orderId": ORDER_ID, "orderNumber": 10, "allocationId": null,
        "amount": "250.00", "customerName": "Klient testowy",
        "occurredAt": "2025-10-24T10:00:00Z", "dueAt": "2025-10-24T10:00:00Z",
        "latestAt": "2025-10-24T10:00:00Z", "isOverdue": false
    })]);
    let settings = Settings {
        api_url: holuto.url().into(),
        token: holuto.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let app = make_app(Arc::clone(&dir), settings.clone(), simulator.link())?;
    let queued = Arc::new(AtomicBool::new(false));
    let fake = Arc::clone(&holuto);
    let printer = Arc::clone(&simulator);
    let sink = move |event: Event| {
        let first_documents = matches!(&event, Event::Documents { items, .. } if !items.is_empty())
            && !queued.swap(true, Ordering::SeqCst);
        let history = matches!(&event, Event::HistoryChanged(entries) if !entries.is_empty());
        let fake = Arc::clone(&fake);
        let printer = Arc::clone(&printer);
        let _scheduled = slint::invoke_from_event_loop(move || {
            with_app(move |app| {
                app.event(event);
                app.sync();
                let window = app.window.as_ref().ok_or("missing window")?.clone_strong();
                if first_documents {
                    control(&window, "10", AccessibleRole::ListItem)
                        .map_err(|error| error.to_string())?
                        .invoke_accessible_default_action();
                    button(&window, "Dodaj do wydruku")
                        .map_err(|error| error.to_string())?
                        .invoke_accessible_default_action();
                    // Wait for the queued selection and queue callbacks before confirming.
                    with_app(|app| {
                        let window = app.window.as_ref().ok_or("missing confirmation window")?;
                        let confirmation =
                            ElementHandle::find_by_accessible_label(window, "Dodaj do wydruku")
                                .filter(|element| {
                                    element.accessible_role() == Some(AccessibleRole::Button)
                                })
                                .last()
                                .ok_or("missing confirmation")?;
                        confirmation.invoke_accessible_default_action();
                        Ok(())
                    });
                }
                if history {
                    assert_eq!(
                        printer.receipts().len(),
                        1,
                        "one operator confirmation creates one fiscal receipt"
                    );
                    assert_eq!(
                        fake.job(Uuid::from_u128(100))["state"],
                        "done",
                        "Holuto accepts the printed result"
                    );
                    assert_eq!(
                        window
                            .get_history()
                            .row_data(0)
                            .and_then(|row| row.row_data(2))
                            .map(|cell| cell.text.to_string()),
                        Some("1".into()),
                        "operator sees printer-assigned receipt number"
                    );
                    app.stop(true)?;
                }
                Ok(())
            });
        });
    };
    let worker = Worker::spawn(dir, settings, simulator.link(), sink)?;
    let mut state = app.borrow_mut();
    state.worker = Some(worker);
    state.model.running = true;
    drop(state);
    slint::run_event_loop_until_quit()?;
    assert!(
        !app.borrow().model.running,
        "quit waits for Stopped and joins the worker"
    );
    assert_eq!(
        simulator.receipts().len(),
        1,
        "the queued document leaves one printed receipt"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[cfg(feature = "simulator")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "End-to-end tests assert while propagating fixture errors"
)]
#[expect(
    clippy::too_many_lines,
    reason = "Real UI confirmation, worker dispatch and durable uncertainty stay in one scenario"
)]
fn overdue_elzab_daily_report_is_available_and_leaves_an_unresolved_block()
-> Result<(), Box<dyn Error>> {
    use elzab::{frame, simulator::Simulator as ElzabSimulator};

    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Arc::new(ElzabSimulator::start());
    simulator.set_status(
        0,
        (1_u128 << 115) | (1_u128 << 102) | (1_u128 << 98) | 4,
        32,
    );
    let holuto = Arc::new(Holuto::new());
    let job_id = holuto.enqueue(testing::job_json());
    let settings = Settings {
        api_url: holuto.url().into(),
        token: holuto.token().into(),
        port: simulator.address().into(),
        vendor: Some(Vendor::Elzab),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let app = make_app(Arc::clone(&dir), settings.clone(), simulator.link())?;
    let requested = AtomicBool::new(false);
    let worker = Worker::spawn(Arc::clone(&dir), settings, simulator.link(), move |event| {
        let request = matches!(&event,
            Event::Device { status: Some(status), blocked: false, .. } if !status.ready())
            && !requested.swap(true, Ordering::SeqCst);
        let complete = matches!(&event, Event::CommandCompleted);
        Application::worker_event(event);
        if request || complete {
            let _scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    let window = app.window.as_ref().ok_or("missing window")?.clone_strong();
                    if request {
                        window.set_tab(1);
                        app.capture();
                        app.sync();
                        assert_eq!(
                            button(&window, "Drukuj raport dobowy")
                                .map_err(|error| error.to_string())?
                                .accessible_enabled(),
                            Some(true)
                        );
                        for label in ["Drukuj raport miesięczny", "Drukuj raport okresowy"] {
                            assert_eq!(
                                button(&window, label)
                                    .map_err(|error| error.to_string())?
                                    .accessible_enabled(),
                                Some(false)
                            );
                        }
                        button(&window, "Drukuj raport dobowy")
                            .map_err(|error| error.to_string())?
                            .invoke_accessible_default_action();
                        with_app(|app| {
                            let window = app.window.as_ref().ok_or("missing confirmation")?;
                            assert_eq!(window.get_confirm_title().as_str(), "Raport dobowy");
                            button(window, "Drukuj")
                                .map_err(|error| error.to_string())?
                                .invoke_accessible_default_action();
                            Ok(())
                        });
                    }
                    if complete {
                        assert!(app.model.blocked);
                        assert!(app.model.report_unresolved);
                        assert_eq!(
                            button(&window, "Drukuj raport dobowy")
                                .map_err(|error| error.to_string())?
                                .accessible_enabled(),
                            Some(false)
                        );
                        app.stop(true)?;
                    }
                    Ok(())
                });
            });
        }
    })?;
    let mut state = app.borrow_mut();
    state.worker = Some(worker);
    state.model.running = true;
    drop(state);
    slint::run_event_loop_until_quit()?;
    assert!(app.borrow().worker.is_none());
    assert_eq!(simulator.reports(), 0, "daily acceptance is not completion");
    let commands = simulator
        .frames()
        .iter()
        .map(|raw| frame::decode(raw).map(|(_, body)| body.to_vec()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        commands
            .iter()
            .filter(|body| body.starts_with(b"RRP"))
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
        [b"RRP\x1dI1\x1dJ1"]
    );
    assert!(simulator.daily_pending());
    assert_eq!(simulator.receipts(), 0);
    assert_eq!(holuto.job(job_id)["state"], "queued");
    assert!(
        !holuto
            .requests()
            .iter()
            .any(|request| request.path.contains("/take"))
    );
    let operation: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("operation.json"))?)?;
    assert_eq!(
        operation["report"]["device"],
        json!({"dispatching": {"stage": "RRP"}})
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[test]
fn quitting_during_a_held_print_waits_for_print_and_settlement() -> Result<(), Box<dyn Error>> {
    held_print_quit()
}

#[cfg(target_os = "macos")]
#[test]
fn native_quit_callback_requests_cooperative_shutdown_during_a_held_print()
-> Result<(), Box<dyn Error>> {
    held_print_quit()
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "End-to-end tests assert while propagating fixture errors"
)]
fn held_print_quit() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Arc::new(Simulator::start());
    simulator.set_clock("2025-10-24T10:00:00Z".parse()?);
    let held = simulator.hold("trend");
    let holuto = Arc::new(Holuto::new());
    let job = holuto.enqueue(testing::job_json());
    let settings = Settings {
        api_url: holuto.url().into(),
        token: holuto.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let app = make_app(Arc::clone(&dir), settings.clone(), simulator.link())?;
    let worker = Worker::spawn(dir, settings, simulator.link(), |event| {
        let _scheduled = slint::invoke_from_event_loop(move || {
            with_app(move |app| {
                app.event(event);
                Ok(())
            });
        });
    })?;
    let mut state = app.borrow_mut();
    state.worker = Some(worker);
    state.model.running = true;
    drop(state);
    let printer = Arc::clone(&simulator);
    let fake = Arc::clone(&holuto);
    let coordinator = thread::Builder::new()
        .name("held-print observer".into())
        .spawn(move || {
            assert!(
                held.reached(Duration::from_secs(10)),
                "receipt reaches held final fiscal command"
            );
            let _scheduled = slint::invoke_from_event_loop(move || {
                #[cfg(target_os = "macos")]
                {
                    let object = NSObject::new();
                    let reply = macos::should_terminate(
                        &object,
                        sel!(applicationShouldTerminate:),
                        &object,
                    );
                    assert_eq!(
                        reply,
                        NSApplicationTerminateReply::TerminateLater,
                        "AppKit holds termination until the worker finishes; Cancel aborts logout"
                    );
                }
                with_app(move |app| {
                    #[cfg(not(target_os = "macos"))]
                    app.stop(true)?;
                    assert!(
                        app.model.stopping,
                        "native and tray quit both request cooperative stop"
                    );
                    assert!(
                        app.worker.is_some(),
                        "quit cannot join or abandon an in-flight worker"
                    );
                    assert!(
                        printer.receipts().is_empty(),
                        "the final command is still held"
                    );
                    assert_eq!(
                        fake.job(job)["state"],
                        "taken",
                        "result cannot precede fiscal completion"
                    );
                    held.release();
                    Ok(())
                });
            });
        })?;
    slint::run_event_loop_until_quit()?;
    coordinator
        .join()
        .map_err(|_panic| "held-print coordinator panicked")?;
    assert!(
        app.borrow().worker.is_none(),
        "Stopped permits the join before exit"
    );
    assert_eq!(
        simulator.receipts().len(),
        1,
        "cooperative quit finishes exactly one receipt"
    );
    assert_eq!(
        holuto.job(job)["state"],
        "done",
        "cooperative quit settles the printed result"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

/// A lost display server (logout) ends the event loop without `finish_quit`.
#[cfg(not(target_os = "macos"))]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "End-to-end tests assert while propagating fixture errors"
)]
fn event_loop_ending_without_quit_still_finishes_the_held_print() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Arc::new(Simulator::start());
    simulator.set_clock("2025-10-24T10:00:00Z".parse()?);
    let held = simulator.hold("trend");
    let holuto = Arc::new(Holuto::new());
    let job = holuto.enqueue(testing::job_json());
    let settings = Settings {
        api_url: holuto.url().into(),
        token: holuto.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let app = make_app(Arc::clone(&dir), settings.clone(), simulator.link())?;
    let worker = Worker::spawn(dir, settings, simulator.link(), Application::worker_event)?;
    let mut state = app.borrow_mut();
    state.worker = Some(worker);
    state.model.running = true;
    drop(state);
    let (loop_ended, ended) = mpsc::channel();
    let coordinator = thread::Builder::new()
        .name("display loss".into())
        .spawn(move || {
            assert!(
                held.reached(Duration::from_secs(10)),
                "receipt reaches held final fiscal command"
            );
            let _scheduled = slint::invoke_from_event_loop(|| {
                let _event_loop_closed = slint::quit_event_loop();
            });
            assert_eq!(
                ended.recv_timeout(Duration::from_secs(10)),
                Ok(()),
                "the event loop ends while the print is held"
            );
            held.release();
        })?;
    slint::run_event_loop_until_quit()?;
    loop_ended.send(())?;
    app.borrow_mut().finish_after_event_loop();
    coordinator
        .join()
        .map_err(|_panic| "display-loss coordinator panicked")?;
    assert!(
        app.borrow().worker.is_none(),
        "exit never leaves the worker running"
    );
    assert_eq!(
        simulator.receipts().len(),
        1,
        "the in-flight receipt completes exactly once"
    );
    assert_eq!(
        holuto.job(job)["state"],
        "done",
        "the printed result settles before exit"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "End-to-end tests assert while propagating fixture errors"
)]
#[expect(
    clippy::too_many_lines,
    reason = "A real lifecycle scenario preserves cross-thread persistence and UI ordering"
)]
fn status_tracks_real_worker_and_quit_waits_for_stopped_publication() -> Result<(), Box<dyn Error>>
{
    use std::sync::{Mutex, mpsc};

    use agent::status::Snapshot;
    use posnet::simulator::{Fault, UNIQUE_NUMBER};

    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Arc::new(Simulator::start());
    simulator.set_clock("2025-10-24T10:00:00Z".parse()?);
    simulator.fault("dailyrep", Fault::CorruptReply);
    let holuto = Holuto::new();
    holuto.documents(vec![json!({
        "kind": "sale", "orderId": ORDER_ID, "orderNumber": 10, "allocationId": null,
        "amount": "250.00", "customerName": "private-customer-canary",
        "occurredAt": "2025-10-24T10:00:00Z", "dueAt": "2025-10-24T10:00:00Z",
        "latestAt": "2025-10-24T10:00:00Z", "isOverdue": false
    })]);
    let settings = Settings {
        api_url: holuto.url().into(),
        token: holuto.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let app = Rc::new(RefCell::new(Application::new(
        Arc::clone(&dir),
        settings.clone(),
        simulator.link(),
        Date::constant(2025, 10, 24),
    )));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    let snapshots = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&snapshots);
    let observed_dir = Arc::clone(&dir);
    let private_url = settings.api_url.clone();
    let private_token = settings.token.as_str().to_owned();
    let (final_reached, final_wait) = mpsc::channel();
    let (release_final, resume_final) = mpsc::channel();
    app.borrow_mut().settings_writer = Some(preferences::Writer::spawn(
        Arc::clone(&dir),
        move |completion| {
            if let Ok(preferences::Outcome::Status(snapshot)) = &completion {
                assert_eq!(
                    Snapshot::load_at(observed_dir.path()).expect("read persisted status"),
                    Some(snapshot.clone()),
                    "a status acknowledgement follows durable persistence"
                );
                let raw = fs::read_to_string(observed_dir.path().join("status.json"))
                    .expect("read status boundary");
                for private in [
                    private_url.as_str(),
                    private_token.as_str(),
                    "private-customer-canary",
                    "customerName",
                    "orderId",
                    "amount",
                    "apiUrl",
                    "token",
                ] {
                    assert!(!raw.contains(private), "status must not expose {private}");
                }
                observed
                    .lock()
                    .expect("snapshot observations")
                    .push(snapshot.clone());
                if !snapshot.processing && snapshot.blocked {
                    final_reached.send(()).expect("announce final status");
                    resume_final.recv().expect("release final acknowledgement");
                }
            }
            let _scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    app.settings_saved(completion);
                    Ok(())
                });
            });
        },
    )?);
    app.borrow_mut().launch()?;
    let report_requested = AtomicBool::new(false);
    let quit_requested = AtomicBool::new(false);
    let worker = Worker::spawn(Arc::clone(&dir), settings, simulator.link(), move |event| {
        let report = matches!(&event,
            Event::Device { status: Some(status), blocked: false, .. } if status.ready())
            && !report_requested.swap(true, Ordering::SeqCst);
        let quit = matches!(
            &event,
            Event::Device {
                blocked: true,
                report_unresolved: true,
                ..
            }
        ) && !quit_requested.swap(true, Ordering::SeqCst);
        Application::worker_event(event);
        if report || quit {
            let _scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    let window = app.window.as_ref().ok_or("missing window")?.clone_strong();
                    if report {
                        window.set_tab(1);
                        app.capture();
                        app.sync();
                        button(&window, "Drukuj raport dobowy")
                            .map_err(|error| error.to_string())?
                            .invoke_accessible_default_action();
                        with_app(|app| {
                            let window = app.window.as_ref().ok_or("missing confirmation")?;
                            button(window, "Drukuj")
                                .map_err(|error| error.to_string())?
                                .invoke_accessible_default_action();
                            Ok(())
                        });
                    }
                    if quit {
                        window.set_tab(3);
                        app.capture();
                        app.sync();
                        button(&window, "Zakończ")
                            .map_err(|error| error.to_string())?
                            .invoke_accessible_default_action();
                    }
                    Ok(())
                });
            });
        }
    })?;
    let mut state = app.borrow_mut();
    state.worker = Some(worker);
    state.model.running = true;
    state.publish_status();
    drop(state);
    let coordinator = thread::Builder::new()
        .name("final status acknowledgement observer".into())
        .spawn(move || {
            let reached = final_wait.recv_timeout(Duration::from_secs(10));
            let scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    assert!(app.quitting);
                    assert!(app.worker.is_none(), "worker has stopped and been joined");
                    assert!(
                        !app.writes_pending.is_empty(),
                        "quit still waits for status"
                    );
                    assert!(
                        app.status_settled
                            .as_ref()
                            .is_some_and(|saved| saved.processing),
                        "a persisted but unacknowledged snapshot is not optimistically accepted"
                    );
                    release_final.send(()).map_err(|error| error.to_string())?;
                    Ok(())
                });
            });
            reached.map_err(|error| error.to_string())?;
            scheduled.map_err(|error| error.to_string())
        })?;
    slint::run_event_loop_until_quit()?;
    coordinator
        .join()
        .map_err(|_panic| "status observer panicked")??;
    let snapshots = snapshots
        .lock()
        .map_err(|_poison| "snapshot observations poisoned")?;
    assert_eq!(
        snapshots
            .iter()
            .map(|status| (
                status.processing,
                status.connected,
                status.blocked,
                status.report_unresolved,
                status.orphaned_receipt,
                status.vendor,
            ))
            .collect::<Vec<_>>(),
        [
            (false, false, false, false, false, None),
            (true, false, false, false, false, None),
            (true, true, false, false, false, None),
            (true, true, false, false, false, Some(Vendor::Posnet)),
            (true, true, true, true, false, Some(Vendor::Posnet)),
            (false, false, true, true, false, Some(Vendor::Posnet)),
        ],
        "state changes publish once; duplicate device polls and acknowledgements do not"
    );
    for snapshot in snapshots.iter().skip(2) {
        assert_eq!(snapshot.unique_number.as_deref(), Some(UNIQUE_NUMBER));
        assert_eq!(
            snapshot.printer_id.as_deref(),
            Some("00000000-0000-0000-0000-000000000009")
        );
    }
    assert_eq!(
        simulator.reports().len(),
        1,
        "uncertain report is never retried"
    );
    assert_eq!(
        Snapshot::load_at(temporary.path())?,
        snapshots.last().cloned(),
        "quit leaves the acknowledged stopped snapshot"
    );
    assert!(app.borrow().writes_pending.is_empty());
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}
