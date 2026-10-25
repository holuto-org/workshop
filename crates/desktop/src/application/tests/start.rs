use agent::testing::{Fault, Holuto};

use super::*;
#[test]
fn saved_start_on_launch_runs_the_same_connection_preflight() -> Result<(), Box<dyn Error>> {
    connect_and_stop(true)
}

#[test]
fn sign_in_saves_settings_and_stops_cooperatively_while_connecting() -> Result<(), Box<dyn Error>> {
    connect_and_stop(false)
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
    let app = make_app(dir, settings.clone(), simulator.link())?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing window")?
        .clone_strong();
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
                    app.stop(true)?;
                    assert!(
                        app.worker.is_some(),
                        "stop waits instead of joining an active connection"
                    );
                    release.send(()).map_err(|error| error.to_string())?;
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
