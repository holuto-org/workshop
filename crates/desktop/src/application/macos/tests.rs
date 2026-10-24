use std::{error::Error, thread, time::Duration};

use agent::{
    settings::Settings,
    store::StateDir,
    testing::{self, Holuto},
};
use jiff::civil::Date;
use objc2::{MainThreadMarker, runtime::NSObjectProtocol, sel};
use objc2_app_kit::NSApplication;
use posnet::simulator::{Hold, Simulator};
use tempfile::TempDir;
use uuid::Uuid;

use super::{APPLICATION, Application, Arc, Rc, RefCell, macos, with_app};

#[expect(
    clippy::panic_in_result_fn,
    reason = "One native process scenario asserts AppKit dispatch and fiscal shutdown ordering"
)]
pub(crate) fn run() -> Result<(), Box<dyn Error>> {
    let main_thread =
        MainThreadMarker::new().ok_or("native test must run on the real main thread")?;
    assert!(
        !macos::install(),
        "installation before winit creates its delegate must fail without caching failure"
    );
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;
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
        telemetry_enabled: false,
        ..Settings::default()
    };
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let app = Rc::new(RefCell::new(Application::new(
        dir,
        settings,
        simulator.link(),
        Date::constant(2025, 10, 24),
    )));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    app.borrow_mut().open()?;
    assert!(
        macos::install(),
        "the actual winit delegate accepts our hook"
    );
    assert!(
        macos::install(),
        "installation is idempotent for our own hook"
    );
    let native = NSApplication::sharedApplication(main_thread);
    let delegate = native
        .delegate()
        .ok_or("winit delegate missing after window startup")?;
    assert!(
        delegate.respondsToSelector(sel!(applicationShouldTerminate:)),
        "the runtime delegate must expose the termination selector, not just a Rust callback"
    );
    app.borrow_mut().start();

    let printer = Arc::clone(&simulator);
    let fake = Arc::clone(&holuto);
    #[expect(
        clippy::expect_used,
        reason = "Failure to dispatch to the native event loop fails the scenario"
    )]
    let coordinator = thread::Builder::new()
        .name("native quit observer".into())
        .spawn(move || {
            if !held.reached(Duration::from_secs(10)) {
                slint::invoke_from_event_loop(|| {
                    let _closed = slint::quit_event_loop();
                })
                .expect("dispatch failure cleanup to the native event loop");
                return false;
            }
            slint::invoke_from_event_loop(move || {
                request_native_quit(held, printer, fake, job);
            })
            .expect("dispatch native termination to the real main thread");
            true
        })?;
    slint::run_event_loop_until_quit()?;
    assert!(
        coordinator
            .join()
            .map_err(|_panic| "native quit coordinator panicked")?,
        "production Worker must reach the held final receipt command"
    );
    assert!(
        app.borrow().quitting,
        "only the native quit request ends this scenario"
    );
    assert!(
        app.borrow().worker.is_none() && !app.borrow().model.running,
        "the event loop exits only after Event::Stopped has joined the worker"
    );
    assert_eq!(
        simulator.receipts().len(),
        1,
        "native quit completes exactly one receipt"
    );
    assert_eq!(
        holuto.job(job)["state"],
        "done",
        "Holuto accepts the completed print before exit"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

#[expect(
    clippy::expect_used,
    reason = "A native dispatch failure must fail this standalone scenario"
)]
fn request_native_quit(held: Hold, printer: Arc<Simulator>, fake: Arc<Holuto>, job: Uuid) {
    let main_thread = MainThreadMarker::new().expect("AppKit callback on main thread");
    let native = NSApplication::sharedApplication(main_thread);
    assert!(
        native.isRunning(),
        "termination is requested during the native event loop"
    );
    native.terminate(None);
    // Another dispatched turn proves terminate returned without ending the event loop.
    slint::invoke_from_event_loop(move || {
        with_app(move |app| {
            assert!(
                app.quitting,
                "AppKit routes terminate through Application::stop"
            );
            assert!(app.model.stopping, "native quit requests cooperative stop");
            assert!(
                app.worker.is_some(),
                "the held worker cannot be joined or abandoned"
            );
            assert!(
                printer.receipts().is_empty(),
                "the fiscal command remains held"
            );
            assert_eq!(
                fake.job(job)["state"],
                "taken",
                "no successful result may precede fiscal completion"
            );
            held.release();
            Ok(())
        });
    })
    .expect("event loop survives native termination while the print is held");
}
