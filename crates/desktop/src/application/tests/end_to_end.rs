use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

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
        "occurredAt": "2025-10-24T10:00:00Z"
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
    // Cross-thread event ownership and UI dispatch are identical to Application::start.
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
                    // Selection and queue callbacks are re-entrancy-safe queued mutations.
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
                        NSApplicationTerminateReply::TerminateCancel,
                        "AppKit cannot terminate before the worker finishes"
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
