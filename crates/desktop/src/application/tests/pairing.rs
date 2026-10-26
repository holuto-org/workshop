use std::{io, path::Path, sync::atomic::AtomicBool};

use agent::testing::{Fault, Holuto};
use i_slint_backend_testing::ElementQuery;

use super::*;

fn rendered(window: &AppWindow, text: &str) -> bool {
    ElementQuery::from_root(window)
        .match_descendants()
        .match_predicate({
            let text = text.to_owned();
            move |element| {
                [
                    element.accessible_label(),
                    element.accessible_value(),
                    element.accessible_description(),
                ]
                .into_iter()
                .flatten()
                .any(|shown| shown.contains(text.as_str()))
            }
        })
        .find_first()
        .is_some()
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "UI assertions propagate fallible fixture setup"
)]
fn pairing_saves_credentials_without_opening_a_printer_and_quit_waits_for_completion()
-> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let fake = Holuto::new();
    fake.pairing_code("ABCD1234");
    let (reached, wait) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    fake.fault(
        "POST",
        "/agent/pair",
        Fault::Hold {
            reached,
            release: resume,
        },
    );
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let simulator = Simulator::start();
    let app = make_app(
        dir,
        Settings {
            api_url: fake.url().into(),
            token: "previous-secret-never-rendered".into(),
            port: "invalid-port-that-must-not-be-opened".into(),
            ..Settings::default()
        },
        simulator.link(),
    )?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing window")?
        .clone_strong();
    assert!(rendered(&window, fake.url()), "the scan sees field values");
    assert!(
        !rendered(&window, "previous-secret-never-rendered"),
        "a stored token is never shown"
    );
    control(&window, "Kod parowania", AccessibleRole::TextInput)?.set_accessible_value("abcd-1234");
    button(&window, "Sparuj")?.invoke_accessible_default_action();
    assert_eq!(button(&window, "Sparuj")?.accessible_enabled(), Some(false));
    let coordinator = thread::Builder::new()
        .name("pairing observer".into())
        .spawn(move || {
            let arrived = wait.recv_timeout(Duration::from_secs(10));
            let scheduled = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    assert!(app.model.pairing);
                    app.stop(true)?;
                    assert!(
                        app.worker.is_some(),
                        "quit waits for pairing, never abandons it"
                    );
                    release.send(()).map_err(|error| error.to_string())?;
                    Ok(())
                });
            });
            arrived.map_err(|error| error.to_string())?;
            scheduled.map_err(|error| error.to_string())
        })?;
    slint::run_event_loop_until_quit()?;
    coordinator
        .join()
        .map_err(|_panic| "pairing observer panicked")??;
    let saved = Settings::load(&app.borrow().dir)?.ok_or("missing paired settings")?;
    assert_eq!(saved.token.as_str(), format!("{:0>42}A", 1));
    window.invoke_sort_documents(0, false);
    assert_eq!(app.borrow().model.settings.token, saved.token);
    assert_eq!(saved.port, "invalid-port-that-must-not-be-opened");
    assert_eq!(window.get_pairing_code().as_str(), "");
    assert_eq!(
        window.get_status().as_str(),
        "Sparowano z Holuto. Możesz uruchomić obsługę drukarki."
    );
    assert!(!app.borrow().model.connected);
    assert!(app.borrow().worker.is_none());
    assert_eq!(
        fake.requests()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        ["/agent/pair"]
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "UI assertions propagate fallible fixture setup"
)]
fn invalid_pairing_code_keeps_credentials_and_allows_operator_correction()
-> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let fake = Holuto::new();
    let simulator = Simulator::start();
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let settings = Settings {
        api_url: fake.url().into(),
        token: "existing-paired-token".into(),
        ..Settings::default()
    };
    settings.save(&dir)?;
    let app = make_app(dir, settings.clone(), simulator.link())?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing window")?
        .clone_strong();
    control(&window, "Kod parowania", AccessibleRole::TextInput)?.set_accessible_value("ZZZZ-ZZZZ");
    button(&window, "Sparuj")?.invoke_accessible_default_action();
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    assert_eq!(Settings::load(&app.borrow().dir)?, Some(settings));
    assert_eq!(window.get_pairing_code().as_str(), "ZZZZ-ZZZZ");
    assert_eq!(button(&window, "Sparuj")?.accessible_enabled(), Some(true));
    assert!(!app.borrow().model.pairing);
    assert_eq!(window.get_status_tone(), 2);
    assert_eq!(
        window.get_status().as_str(),
        concat!(
            "Nieprawidłowy, wykorzystany lub wygasły kod parowania. ",
            "Wygeneruj nowy kod w Holuto."
        )
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "UI assertions propagate fallible fixture setup"
)]
fn malformed_pairing_code_is_rejected_locally_and_remains_editable() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let fake = Holuto::new();
    let simulator = Simulator::start();
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let settings = Settings {
        api_url: fake.url().into(),
        token: "existing-paired-token".into(),
        ..Settings::default()
    };
    settings.save(&dir)?;
    let app = make_app(dir, settings.clone(), simulator.link())?;
    let window = app
        .borrow()
        .window
        .as_ref()
        .ok_or("missing window")?
        .clone_strong();
    control(&window, "Kod parowania", AccessibleRole::TextInput)?.set_accessible_value("ABCD-123I");
    button(&window, "Sparuj")?.invoke_accessible_default_action();
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    assert_eq!(
        window.get_status().as_str(),
        "Kod parowania ma 8 znaków z Holuto, na przykład ABCD-1234."
    );
    assert_eq!(window.get_pairing_code().as_str(), "ABCD-123I");
    assert_eq!(button(&window, "Sparuj")?.accessible_enabled(), Some(true));
    assert_eq!(
        control(&window, "Kod parowania", AccessibleRole::TextInput)?.accessible_enabled(),
        Some(true)
    );
    assert_eq!(Settings::load(&app.borrow().dir)?, Some(settings));
    assert!(
        fake.requests().is_empty(),
        "malformed codes never reach HTTP"
    );
    assert!(
        simulator.frames().is_empty(),
        "pairing never opens a printer"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

/// Runs `step` once `ready` holds and no settings or status write is pending, re-queued on
/// the event loop behind the writer's completions instead of waiting on the wall clock.
fn when<R, F>(ready: R, step: F)
where
    R: Fn(&Application) -> bool + Send + 'static,
    F: FnOnce(&mut Application) -> Result<(), slint::PlatformError> + Send + 'static,
{
    let _closed = slint::invoke_from_event_loop(move || {
        with_app(move |app| {
            if app.model.saving || !app.writes_pending.is_empty() || !ready(app) {
                when(ready, step);
                return Ok(());
            }
            step(app)
        });
    });
}

/// A directory in place of `settings.json` fails the reload on every platform and user.
fn break_settings(file: &Path, aside: &Path) -> io::Result<()> {
    fs::rename(file, aside)?;
    fs::create_dir(file)
}

fn restore_settings(file: &Path, aside: &Path) -> io::Result<()> {
    fs::remove_dir(file)?;
    fs::rename(aside, file)
}

fn assert_reload_failed(app: &Application) {
    assert_eq!(app.model.tone, Tone::Error, "{}", app.model.message);
    assert!(
        app.model.message.starts_with(RELOAD_FAILED),
        "{}",
        app.model.message
    );
}

const RELOAD_FAILED: &str = "Nie można odczytać ustawień (";

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "UI assertions propagate fallible fixture setup"
)]
fn save_after_a_failed_reload_keeps_the_credentials_pairing_saved() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().join("state"))?);
    let unpaired = Settings {
        api_url: "https://paired.example.test".into(),
        ..Settings::default()
    };
    unpaired.save(&dir)?;
    let app = make_app(
        Arc::clone(&dir),
        unpaired.clone(),
        Simulator::start().link(),
    )?;
    let paired = Settings {
        token: "synthetic-paired-token".into(),
        ..unpaired
    };
    paired.save(&dir)?;
    let file = temporary.path().join("state").join("settings.json");
    let aside = temporary.path().join("settings.aside");
    break_settings(&file, &aside)?;
    app.borrow_mut().event(Event::Paired {
        printer_id: "printer-42".into(),
    });
    when(
        |_app| true,
        move |app| {
            assert_reload_failed(app);
            restore_settings(&file, &aside).map_err(|error| error.to_string())?;
            app.capture();
            app.action(Action::Save)?;
            app.stop(true)
        },
    );
    slint::run_event_loop_until_quit()?;
    let saved = Settings::load(&dir)?.ok_or("missing settings")?;
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    assert_eq!(
        saved.token, paired.token,
        "a failed reload must not let Save erase the credentials pairing saved"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "UI assertions propagate fallible fixture setup"
)]
fn start_after_a_failed_reload_connects_with_the_credentials_pairing_saved()
-> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let temporary = TempDir::new()?;
    let simulator = Simulator::start();
    let holuto = Holuto::new();
    let dir = Arc::new(StateDir::open(temporary.path().join("state"))?);
    let unpaired = Settings {
        api_url: holuto.url().into(),
        port: simulator.address().into(),
        ..Settings::default()
    };
    unpaired.save(&dir)?;
    let app = make_app(Arc::clone(&dir), unpaired.clone(), simulator.link())?;
    Settings {
        token: holuto.token().into(),
        ..unpaired
    }
    .save(&dir)?;
    let file = temporary.path().join("state").join("settings.json");
    let aside = temporary.path().join("settings.aside");
    break_settings(&file, &aside)?;
    app.borrow_mut().event(Event::Paired {
        printer_id: "printer-42".into(),
    });
    let connected = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&connected);
    when(
        |_app| true,
        move |app| {
            assert_reload_failed(app);
            restore_settings(&file, &aside).map_err(|error| error.to_string())?;
            app.capture();
            app.action(Action::Start)?;
            // Stop once connected, or once a refused Start or ended worker leaves none.
            when(
                |app| app.model.connected || app.worker.is_none(),
                move |app| {
                    observed.store(app.model.connected, Ordering::SeqCst);
                    app.stop(true)
                },
            );
            Ok(())
        },
    );
    slint::run_event_loop_until_quit()?;
    let message = app.borrow().model.message.clone();
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    assert!(
        connected.load(Ordering::SeqCst),
        "Start must use the stored pairing after a failed reload; last status: {message}"
    );
    Ok(())
}
