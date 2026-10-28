use std::sync::atomic::AtomicBool;

#[cfg(target_os = "macos")]
use objc2::{runtime::NSObject, sel};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSApplicationTerminateReply;

use agent::{
    api::model::JobState,
    testing::{self, Holuto, ORDER_ID},
};
use posnet::simulator::SimReceipt;
use serde_json::json;

use super::*;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "End-to-end tests assert while propagating fixture errors"
)]
#[expect(
    clippy::too_many_lines,
    reason = "One UI-to-print lifecycle proves callback ordering and exact server effects"
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
    // Opening the dialog is covered separately; a click here would be a re-entrant callback.
    app.borrow_mut().model.manual = true;
    let queued = Arc::new(AtomicBool::new(false));
    let fake = Arc::clone(&holuto);
    let printer = Arc::clone(&simulator);
    let sink = move |event: Event| {
        let first_documents = matches!(&event, Event::Documents { items, .. } if !items.is_empty())
            && !queued.swap(true, Ordering::SeqCst);
        let history = matches!(
            &event, Event::Jobs { items, .. }
                if items.iter().any(|view| view.job.state == JobState::Done)
        );
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
                    button(&window, "Dalej")
                        .map_err(|error| error.to_string())?
                        .invoke_accessible_default_action();
                    // Wait for the queued selection and queue callbacks before confirming.
                    with_app(|app| {
                        let window = app.window.as_ref().ok_or("missing confirmation window")?;
                        assert_eq!(
                            window.get_confirm_text(),
                            concat!(
                                "Sprzedaż · zlecenie 10 · Klient testowy · 250,00 zł\n",
                                "Drukarka wystawi paragon fiskalny."
                            )
                        );
                        // Type and click from a fresh event-loop task, as an operator does:
                        // inside this callback, its closing sync would overwrite the typed NIP.
                        let window = window.as_weak();
                        let _scheduled = slint::invoke_from_event_loop(move || {
                            let enter = || -> Result<(), String> {
                                let window = window.upgrade().ok_or("missing window")?;
                                let confirmation = ElementHandle::find_by_accessible_label(
                                    &window,
                                    "Wystaw paragon",
                                )
                                .filter(|element| {
                                    element.accessible_role() == Some(AccessibleRole::Button)
                                })
                                .last()
                                .ok_or("missing confirmation")?;
                                let nip =
                                    control(&window, "NIP nabywcy", AccessibleRole::TextInput)
                                        .map_err(|error| error.to_string())?;
                                nip.set_accessible_value("526104082");
                                confirmation.invoke_accessible_default_action();
                                assert_eq!(
                                    (window.get_confirming(), window.get_nip_invalid()),
                                    (true, true),
                                    "an invalid NIP keeps the confirmation open for correction"
                                );
                                nip.set_accessible_value("5261040828");
                                confirmation.invoke_accessible_default_action();
                                confirmation.invoke_accessible_default_action();
                                Ok(())
                            };
                            assert_eq!(enter(), Ok(()));
                        });
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
                    let listed = control(&window, "Zlecenie 10", AccessibleRole::ListItem)
                        .map_err(|error| error.to_string())?
                        .accessible_description()
                        .unwrap_or_default();
                    assert!(
                        listed.contains("Nr 1 · "),
                        "operator sees printer-assigned receipt number: {listed}"
                    );
                    app.stop(true)?;
                }
                Ok(())
            });
        });
    };
    let worker = Worker::spawn(
        dir,
        settings,
        simulator.link(),
        worker::Searches::default(),
        sink,
    )?;
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
    assert_eq!(
        holuto
            .requests()
            .into_iter()
            .filter(|request| request.method == "POST" && request.path == "/agent/jobs")
            .map(|request| request.body)
            .collect::<Vec<_>>(),
        [json!({
            "kind": "sale", "orderId": ORDER_ID, "allocationId": null, "buyerNip": "5261040828"
        })],
        "a second accessible confirmation cannot queue a second fiscal job"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "End-to-end tests assert while propagating fixture errors"
)]
fn a_typing_pause_lists_only_the_matching_holuto_documents() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Simulator::start();
    let holuto = Holuto::new();
    let document = |order: Uuid, number: u8, customer: &str| {
        json!({
            "kind": "sale", "orderId": order, "orderNumber": number, "allocationId": null,
            "amount": "250.00", "customerName": customer,
            "occurredAt": "2025-10-24T10:00:00Z", "dueAt": "2025-10-24T10:00:00Z",
            "latestAt": "2025-10-24T10:00:00Z", "isOverdue": false
        })
    };
    holuto.documents(vec![
        document(ORDER_ID, 10, "Jan Kowalski"),
        document(Uuid::from_u128(77), 11, "Anna Nowak"),
    ]);
    let settings = Settings {
        api_url: holuto.url().into(),
        token: holuto.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let app = make_app(Arc::clone(&dir), settings.clone(), simulator.link())?;
    let typed = Arc::new(AtomicBool::new(false));
    let worker = Worker::spawn(
        dir,
        settings,
        simulator.link(),
        worker::Searches::default(),
        move |event| {
            let listed = matches!(&event, Event::Documents { .. });
            Application::worker_event(event);
            if !listed {
                return;
            }
            let typed = Arc::clone(&typed);
            let _scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    let window = app.window.as_ref().ok_or("missing window")?;
                    let shown: Vec<_> = app
                        .model
                        .documents()
                        .iter()
                        .map(|document| document.order_number)
                        .collect();
                    if window.get_searching() {
                        assert_eq!(shown, [10], "only the matching customer is listed");
                        app.stop(true)?;
                    } else if shown == [10, 11] && !typed.swap(true, Ordering::SeqCst) {
                        button(window, "+ Wystaw paragon")
                            .map_err(|error| error.to_string())?
                            .invoke_accessible_default_action();
                        // Type from a fresh event-loop task, as an operator does: this callback's
                        // closing sync would overwrite the typed text.
                        let window = window.as_weak();
                        let _typing = slint::invoke_from_event_loop(move || {
                            let typing = || -> Result<(), String> {
                                let window = window.upgrade().ok_or("missing window")?;
                                control(&window, "Szukaj dokumentu", AccessibleRole::TextInput)
                                    .map_err(|error| error.to_string())?
                                    .set_accessible_value("kowal");
                                // Starts the debounce timer, as the next event-loop pass would.
                                i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
                                i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(
                                    399,
                                ));
                                assert!(!window.get_searching(), "typing alone does not search");
                                i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(
                                    1,
                                ));
                                assert!(window.get_searching(), "a 400 ms pause searches");
                                Ok(())
                            };
                            assert_eq!(typing(), Ok(()));
                        });
                    }
                    Ok(())
                });
            });
        },
    )?;
    let mut state = app.borrow_mut();
    state.worker = Some(worker);
    state.model.running = true;
    drop(state);
    slint::run_event_loop_until_quit()?;
    let searches: Vec<_> = holuto
        .requests()
        .into_iter()
        .filter(|request| {
            request.path == "/agent/documents/search" && request.body["search"]["q"].is_string()
        })
        .map(|request| request.body)
        .collect();
    assert_eq!(
        searches.first(),
        Some(&json!({"cursor": {"limit": 100}, "filter": {}, "search": {"q": "kowal"}})),
        "the search text travels only in the request body"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Real worker history is asserted through accessible UI"
)]
fn a_typing_pause_lists_only_the_matching_server_jobs() -> Result<(), Box<dyn Error>> {
    use agent::testing::{job_json, with};
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Simulator::start();
    let holuto = Arc::new(Holuto::new());
    for (number, customer, receipt) in [
        (1041, "Anna Nowak", "2186"),
        (1042, "Łukasz Kowalski", "2187"),
    ] {
        let id = Uuid::from_u128(number);
        holuto.insert(with(
            job_json(),
            &json!({
                "id": id, "orderNumber": number, "state": "done", "receiptNumber": receipt,
                "issuedAt": "2025-10-24T10:00:00Z"
            }),
        ));
        holuto.set_job_metadata(
            id,
            &json!({"customerName": customer, "vehiclePlate": "WA12345", "source": "staff"}),
        );
    }
    let settings = Settings {
        api_url: holuto.url().into(),
        token: holuto.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let app = make_app(Arc::clone(&dir), settings.clone(), simulator.link())?;
    let typed = Arc::new(AtomicBool::new(false));
    let fake = Arc::clone(&holuto);
    let worker = Worker::spawn(
        dir,
        settings,
        simulator.link(),
        worker::Searches::default(),
        move |event| {
            let listed = matches!(&event, Event::Jobs { .. });
            Application::worker_event(event);
            if !listed {
                return;
            }
            let typed = Arc::clone(&typed);
            let fake = Arc::clone(&fake);
            let _scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    let window = app.window.as_ref().ok_or("missing window")?;
                    if typed.swap(true, Ordering::SeqCst) {
                        let rows: Vec<_> = window.get_receipts().iter().collect();
                        assert!(
                            matches!(rows.as_slice(), [row] if row.order == "Zlecenie 1042"
                                && row.customer == "Łukasz Kowalski · WA12345"
                                && row.receipt.starts_with("Nr 2187 · 24.10.2025")),
                            "only the matching job is listed: {rows:?}"
                        );
                        return app.stop(true);
                    }
                    // Type from a fresh event-loop task, as an operator does: this callback's
                    // closing sync would overwrite the typed text.
                    let window = window.as_weak();
                    let _typing = slint::invoke_from_event_loop(move || {
                        let typing = || -> Result<(), String> {
                            let window = window.upgrade().ok_or("missing window")?;
                            control(&window, "Szukaj", AccessibleRole::TextInput)
                                .map_err(|error| error.to_string())?
                                .set_accessible_value("kowalski");
                            // Starts the debounce timer, as the next event-loop pass would.
                            i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
                            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(399));
                            assert!(
                                !fake.requests().iter().any(|request| {
                                    request.path == "/agent/jobs/search"
                                        && request.body["search"]["q"].is_string()
                                }),
                                "399 ms must not dispatch the search"
                            );
                            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
                            Ok(())
                        };
                        assert_eq!(typing(), Ok(()));
                    });
                    Ok(())
                });
            });
        },
    )?;
    let mut state = app.borrow_mut();
    state.worker = Some(worker);
    state.model.running = true;
    drop(state);
    slint::run_event_loop_until_quit()?;
    let searches: Vec<_> = holuto
        .requests()
        .into_iter()
        .filter(|request| request.path == "/agent/jobs/search")
        .filter_map(|request| request.body["search"]["q"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(searches, ["kowalski"], "only the paused text is searched");
    assert_eq!(simulator.receipts(), Vec::<SimReceipt>::new());
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Paging and unavailable recovery are real UI-worker-HTTP behavior"
)]
#[expect(
    clippy::too_many_lines,
    reason = "The scenario preserves paging state through a server outage and reset"
)]
fn history_pages_clear_on_failure_and_retry_restarts_at_first_page() -> Result<(), Box<dyn Error>> {
    use agent::testing::Fault;
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Simulator::start();
    let holuto = Arc::new(Holuto::new());
    holuto.jobs_page(
        None,
        json!({
            "items": [receipt_view_json(10, "done")],
            "nextCursor": "second",
            "previousCursor": null
        }),
    );
    holuto.jobs_page(
        Some("first"),
        json!({
            "items": [receipt_view_json(10, "done")],
            "nextCursor": "second",
            "previousCursor": null
        }),
    );
    holuto.jobs_page(
        Some("second"),
        json!({
            "items": [receipt_view_json(11, "expired")],
            "nextCursor": null,
            "previousCursor": "first"
        }),
    );
    let settings = Settings {
        api_url: holuto.url().into(),
        token: holuto.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let app = make_app(Arc::clone(&dir), settings.clone(), simulator.link())?;
    let phase = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&phase);
    let fake = Arc::clone(&holuto);
    let worker = Worker::spawn(
        dir,
        settings,
        simulator.link(),
        worker::Searches::default(),
        move |event| {
            let unavailable = matches!(&event, Event::JobsUnavailable);
            let history = matches!(&event, Event::Jobs { .. })
                || (matches!(&event, Event::JobsUnavailable)
                    && observed.load(Ordering::SeqCst) > 0);
            Application::worker_event(event);
            if history {
                let phase = Arc::clone(&observed);
                let fake = Arc::clone(&fake);
                let _scheduled = slint::invoke_from_event_loop(move || {
                    with_app(move |app| {
                        let window = app.window.as_ref().ok_or("missing window")?.clone_strong();
                        if unavailable && phase.load(Ordering::SeqCst) != 1 {
                            return Ok(());
                        }
                        let step = phase.fetch_add(1, Ordering::SeqCst);
                        let action = match step {
                            0 => {
                                assert_eq!(window.get_jobs_page_label(), "Strona 1");
                                fake.fault(
                                    "POST",
                                    "/agent/jobs/search",
                                    Fault::Problem {
                                        status: 500,
                                        code: "internal_error".into(),
                                        detail: "synthetic unavailable".into(),
                                    },
                                );
                                "Następna strona historii"
                            },
                            1 => {
                                assert!(window.get_jobs_unavailable());
                                assert_eq!(window.get_receipts().row_count(), 0);
                                assert_eq!(window.get_receipt_detail().order, "");
                                assert!(
                                    !window.get_jobs_previous_enabled()
                                        && !window.get_jobs_next_enabled()
                                );
                                "Ponów pobieranie historii"
                            },
                            2 => {
                                assert!(!window.get_jobs_unavailable());
                                assert_eq!(window.get_jobs_page_label(), "Strona 1");
                                "Następna strona historii"
                            },
                            3 => {
                                assert_eq!(window.get_jobs_page_label(), "Strona 2");
                                assert_eq!(
                                    window
                                        .get_receipts()
                                        .row_data(0)
                                        .ok_or("missing second page")?
                                        .order,
                                    "Zlecenie 11"
                                );
                                "Poprzednia strona historii"
                            },
                            4 => {
                                assert_eq!(window.get_jobs_page_label(), "Strona 1");
                                "Historia od początku"
                            },
                            _ => {
                                assert_eq!(window.get_jobs_page_label(), "Strona 1");
                                app.stop(true)?;
                                return Ok(());
                            },
                        };
                        button(&window, action)
                            .map_err(|error| error.to_string())?
                            .invoke_accessible_default_action();
                        Ok(())
                    });
                });
            }
        },
    )?;
    let mut state = app.borrow_mut();
    state.worker = Some(worker);
    state.model.running = true;
    drop(state);
    slint::run_event_loop_until_quit()?;
    assert_eq!(phase.load(Ordering::SeqCst), 6);
    assert_eq!(simulator.receipts(), Vec::<SimReceipt>::new());
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
    let worker = Worker::spawn(
        Arc::clone(&dir),
        settings,
        simulator.link(),
        worker::Searches::default(),
        move |event| {
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
        },
    )?;
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
    let worker = Worker::spawn(
        dir,
        settings,
        simulator.link(),
        worker::Searches::default(),
        |event| {
            let _scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    app.event(event);
                    Ok(())
                });
            });
        },
    )?;
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
                request_native_termination();
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

/// Sends `AppKit`'s termination request through the installed delegate hook.
#[cfg(target_os = "macos")]
fn request_native_termination() {
    let object = NSObject::new();
    let reply = macos::should_terminate(&object, sel!(applicationShouldTerminate:), &object);
    assert_eq!(
        reply,
        NSApplicationTerminateReply::TerminateLater,
        "AppKit holds termination until the worker finishes; Cancel aborts logout"
    );
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
    let worker = Worker::spawn(
        dir,
        settings,
        simulator.link(),
        worker::Searches::default(),
        Application::worker_event,
    )?;
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
        || Date::constant(2025, 10, 24),
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
    let worker = Worker::spawn(
        Arc::clone(&dir),
        settings,
        simulator.link(),
        worker::Searches::default(),
        move |event| {
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
                            window.set_tab(2);
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
        },
    )?;
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
