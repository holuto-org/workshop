use agent::testing::{Fault, Holuto};
use serde_json::json;

use super::*;
#[test]
fn saved_start_on_launch_runs_the_same_connection_preflight() -> Result<(), Box<dyn Error>> {
    connect_and_stop(true)
}

#[test]
fn sign_in_saves_settings_and_stops_cooperatively_while_connecting() -> Result<(), Box<dyn Error>> {
    connect_and_stop(false)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real worker and Holuto fake prove the typed search reaches a new session"
)]
fn a_new_worker_uses_both_typed_searches_for_its_first_lists() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Simulator::start();
    let fake = Arc::new(Holuto::new());
    let (reached, arrivals) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    fake.fault(
        "POST",
        "/agent/jobs/search",
        Fault::Hold {
            reached: reached.clone(),
            release: resume,
        },
    );
    let (document_release, document_resume) = mpsc::channel();
    fake.fault(
        "POST",
        "/agent/documents/search",
        Fault::Hold {
            reached,
            release: document_resume,
        },
    );
    let settings = Settings {
        api_url: fake.url().into(),
        token: fake.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    settings.save(&dir)?;
    let app = make_app(dir, settings, simulator.link())?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing window")?
        .clone_strong();
    // Typed before Start, or left typed across Stop: no worker was told yet.
    control(&window, "Szukaj", AccessibleRole::TextInput)?.set_accessible_value("kowal");
    button(&window, "+ Wystaw paragon")?.invoke_accessible_default_action();
    control(&window, "Szukaj dokumentu", AccessibleRole::TextInput)?.set_accessible_value("nowak");
    button(&window, "Zamknij")?.invoke_accessible_default_action();
    window.set_tab(2);
    button(&window, "Uruchom")?.invoke_accessible_default_action();
    let coordinator = thread::Builder::new()
        .name("search observer".into())
        .spawn(move || {
            let jobs_reached = arrivals.recv_timeout(Duration::from_secs(10)).is_ok();
            let _released = release.send(());
            let documents_reached = arrivals.recv_timeout(Duration::from_secs(10)).is_ok();
            let _released = document_release.send(());
            let _scheduled = slint::invoke_from_event_loop(|| {
                with_app(|app| app.stop(true));
            });
            jobs_reached && documents_reached
        })?;
    slint::run_event_loop_until_quit()?;
    let found = coordinator
        .join()
        .map_err(|_panic| "search observer panicked")?;
    assert!(found, "both first listing requests must reach Holuto");
    for (path, search) in [
        ("/agent/documents/search", "nowak"),
        ("/agent/jobs/search", "kowal"),
    ] {
        assert_eq!(
            fake.requests()
                .iter()
                .find(|request| request.path == path)
                .map(|request| request.body["search"].clone()),
            Some(json!({"q": search})),
            "{path} must never first list unfiltered rows"
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
fn stop_while_start_is_saving_settings_cancels_the_start() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Simulator::start();
    let fake = Holuto::new();
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let settings = Settings {
        api_url: fake.url().into(),
        token: fake.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    settings.save(&dir)?;
    let app = Rc::new(RefCell::new(Application::new(
        Arc::clone(&dir),
        settings,
        simulator.link(),
        || Date::constant(2025, 10, 24),
    )));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    let (started, worker_spawned) = mpsc::channel();
    app.borrow_mut().settings_writer = Some(preferences::Writer::spawn(dir, move |completion| {
        let started = started.clone();
        let _scheduled = slint::invoke_from_event_loop(move || {
            with_app(move |app| {
                let start = matches!(completion, Ok(preferences::Outcome::Start(_)));
                app.settings_saved(completion);
                if start {
                    started
                        .send(app.worker.is_some())
                        .map_err(|error| error.to_string())?;
                    app.stop(true)?;
                }
                Ok(())
            });
        });
    })?);
    app.borrow_mut().launch()?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing window")?
        .clone_strong();
    window.set_tab(2);
    button(&window, "Uruchom")?.invoke_accessible_default_action();
    assert!(app.borrow().model.saving, "start waits for its settings");
    // Assistive technology can activate the disabled button.
    button(&window, "Zatrzymaj")?.invoke_accessible_default_action();
    slint::run_event_loop_until_quit()?;
    assert_eq!(
        worker_spawned.try_recv(),
        Ok(false),
        "a Stop before the worker exists cancels the pending start"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn connect_and_stop(start_on_launch: bool) -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Simulator::start();
    let fake = Holuto::new();
    let (reached, wait) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    fake.fault(
        "POST",
        "/agent/session",
        Fault::Hold {
            reached,
            release: resume,
        },
    );
    let settings = Settings {
        api_url: fake.url().into(),
        token: fake.token().into(),
        port: simulator.address().into(),
        start_on_launch,
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    // The application loads its pairing from disk, as `main` does; Start saves the port.
    Settings {
        port: String::new(),
        ..settings.clone()
    }
    .save(&dir)?;
    let app = make_app(dir, settings.clone(), simulator.link())?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing window")?
        .clone_strong();
    window.set_tab(2);
    if !start_on_launch {
        button(&window, "Uruchom")?.invoke_accessible_default_action();
    }
    assert_eq!(
        button(&window, "Uruchom")?.accessible_enabled(),
        Some(false),
        "connection cannot be submitted twice"
    );
    let coordinator = thread::Builder::new()
        .name("connection observer".into())
        .spawn(move || {
            assert!(
                wait.recv_timeout(Duration::from_secs(10)).is_ok(),
                "worker reaches the held session"
            );
            let _scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    assert!(app.model.running, "held connection keeps worker alive");
                    // Tray Quit and signals stop without a window callback capturing first.
                    let window = app.window.as_ref().ok_or("missing window")?.clone_strong();
                    window.set_nip("5261040828".into());
                    app.stop(true)?;
                    assert!(
                        app.worker.is_some(),
                        "stop waits instead of joining an active connection"
                    );
                    with_app(move |app| {
                        let window = app.window.as_ref().ok_or("missing window")?;
                        assert_eq!(
                            window.get_nip().as_str(),
                            "5261040828",
                            "quitting keeps text the operator typed"
                        );
                        release.send(()).map_err(|error| error.to_string())?;
                        Ok(())
                    });
                    Ok(())
                });
            });
        })?;
    slint::run_event_loop_until_quit()?;
    coordinator
        .join()
        .map_err(|_panic| "connection observer panicked")?;
    assert!(
        app.borrow().worker.is_none(),
        "Stopped is observed before the worker is joined"
    );
    assert_eq!(
        Settings::load(&app.borrow().dir)?,
        Some(Settings {
            vendor: Some(Vendor::Posnet),
            ..settings
        }),
        "sign-in settings and detected vendor are durably preserved"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A held real worker protects connection settings against assistive actions"
)]
fn running_worker_refuses_save_start_and_pair_without_changing_its_configuration()
-> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Simulator::start();
    let holuto = Holuto::new();
    let (reached, held) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    holuto.fault(
        "POST",
        "/agent/session",
        Fault::Hold {
            reached,
            release: resume,
        },
    );
    let settings = Settings {
        api_url: holuto.url().into(),
        token: holuto.token().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    settings.save(&dir)?;
    let app = make_app(Arc::clone(&dir), settings.clone(), simulator.link())?;
    let worker = Worker::spawn(
        dir,
        settings.clone(),
        simulator.link(),
        worker::Searches::default(),
        Application::worker_event,
    )?;
    app.borrow_mut().worker = Some(worker);
    app.borrow_mut().model.running = true;
    app.borrow().sync();
    held.recv_timeout(Duration::from_secs(10))?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing window")?
        .clone_strong();
    window.set_tab(2);
    control(&window, "Kod parowania", AccessibleRole::TextInput)?.set_accessible_value("ABCD1234");
    for label in ["Uruchom", "Sparuj"] {
        button(&window, label)?.invoke_accessible_default_action();
        assert!(
            !app.borrow().model.saving,
            "{label} cannot submit a settings write"
        );
        assert!(
            !app.borrow().model.pairing,
            "{label} cannot replace the active worker"
        );
        assert_eq!(app.borrow().model.settings, settings);
    }
    assert_eq!(Settings::load(&app.borrow().dir)?, Some(settings));
    app.borrow_mut().stop(true)?;
    release.send(())?;
    slint::run_event_loop_until_quit()?;
    assert!(app.borrow().worker.is_none());
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Real paging callbacks send only navigation allowed by the page"
)]
fn page_callbacks_reject_disabled_actions_and_navigate_next_then_previous()
-> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Simulator::start();
    let holuto = Arc::new(Holuto::new());
    let document = |number: u32| {
        json!({
            "kind":"sale", "orderId":Uuid::from_u128(u128::from(number)), "orderNumber":number,
            "allocationId":null, "amount":"1.00", "customerName":null,
            "occurredAt":"2025-10-24T10:00:00Z", "dueAt":"2025-10-24T10:00:00Z",
            "latestAt":"2025-10-24T10:00:00Z", "isOverdue":false
        })
    };
    holuto.page(
        None,
        json!({"items":[document(10)],"nextCursor":"second-page","previousCursor":null}),
    );
    holuto.page(
        Some("first"),
        json!({"items":[document(10)],"nextCursor":"second-page","previousCursor":null}),
    );
    holuto.page(
        Some("second-page"),
        json!({"items":[document(11)],"nextCursor":null,"previousCursor":"first"}),
    );
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
    let pages = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&pages);
    let worker = Worker::spawn(
        dir,
        settings,
        simulator.link(),
        worker::Searches::default(),
        move |event| {
            let step = matches!(event, Event::Documents { .. })
                .then(|| observed.fetch_add(1, Ordering::SeqCst));
            let _scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    app.event(event);
                    app.sync();
                    let window = app.window.as_ref().ok_or("missing window")?;
                    if let Some(step) = step {
                        assert_eq!(app.model.page, if step == 1 { 2 } else { 1 });
                        match step {
                            0 => {
                                button(window, "Poprzednia strona")
                                    .map_err(|error| error.to_string())?
                                    .invoke_accessible_default_action();
                                button(window, "Następna strona")
                                    .map_err(|error| error.to_string())?
                                    .invoke_accessible_default_action();
                            },
                            1 => {
                                button(window, "Następna strona")
                                    .map_err(|error| error.to_string())?
                                    .invoke_accessible_default_action();
                                button(window, "Poprzednia strona")
                                    .map_err(|error| error.to_string())?
                                    .invoke_accessible_default_action();
                            },
                            2 => app.stop(true)?,
                            _ => panic!("unexpected page refresh"),
                        }
                    }
                    Ok(())
                });
            });
        },
    )?;
    app.borrow_mut().worker = Some(worker);
    app.borrow_mut().model.running = true;
    slint::run_event_loop_until_quit()?;
    assert_eq!(pages.load(Ordering::SeqCst), 3);
    assert_eq!(
        holuto
            .requests()
            .into_iter()
            .filter(|request| request.path == "/agent/documents/search")
            .map(|request| request.body["cursor"].clone())
            .collect::<Vec<_>>(),
        [
            json!({"limit": 100}),
            json!({"limit": 100, "cursor": "second-page"}),
            json!({"limit": 100, "cursor": "first"})
        ],
        "disabled buttons send nothing; enabled ones follow the server cursors"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}
