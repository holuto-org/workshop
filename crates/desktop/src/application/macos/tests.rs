use std::{
    error::Error,
    ffi::c_void,
    io::{self, Write as _},
    path::Path,
    thread,
    time::Duration,
};

use agent::{
    settings::Settings,
    store::StateDir,
    testing::{self, Holuto},
};
use jiff::civil::Date;
use objc2::{MainThreadMarker, runtime::NSObjectProtocol, sel};
use objc2_app_kit::NSApplication;
use posnet::simulator::{Hold, Simulator};
use uuid::Uuid;

use super::{APPLICATION, Application, Arc, Rc, RefCell, macos, with_app};

/// Printed inside `AppKit`'s nested run loop while it holds the termination.
pub(crate) const HELD: &[u8] = b"native quit held until the worker stopped\n";

/// Never returns on success: `AppKit` ends the process after the held termination's reply.
///
/// The caller proves from `state` that exit followed `Event::Stopped`.
#[expect(
    clippy::panic_in_result_fn,
    reason = "One native process scenario asserts AppKit dispatch and fiscal shutdown ordering"
)]
pub(crate) fn run(state: &Path) -> Result<(), Box<dyn Error>> {
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
    let dir = Arc::new(StateDir::open(state.to_owned())?);
    let app = Rc::new(RefCell::new(Application::new(
        dir,
        settings,
        simulator.link(),
        || Date::constant(2025, 10, 24),
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
            request_native_quit(Box::new(HeldQuit {
                held,
                printer,
                fake,
                job,
            }));
            true
        })?;
    slint::run_event_loop_until_quit()?;
    assert!(
        coordinator
            .join()
            .map_err(|_panic| "native quit coordinator panicked")?,
        "production Worker must reach the held final receipt command"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    // Returning means AppKit refused the termination instead of holding it.
    Err("AppKit termination was refused; the event loop ended instead of AppKit exiting".into())
}

struct HeldQuit {
    held: Hold,
    printer: Arc<Simulator>,
    fake: Arc<Holuto>,
    job: Uuid,
}

#[repr(C)]
struct DispatchQueue {
    _opaque: [u8; 0],
}

#[expect(unsafe_code, reason = "libdispatch has no binding among the dependencies")]
unsafe extern "C" {
    static _dispatch_main_q: DispatchQueue;
    fn dispatch_async_f(
        queue: *const DispatchQueue,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}

/// Requests `terminate:` from a main-queue callback, outside winit's event handler.
///
/// A nested `AppKit` run loop entered inside a Slint event could never deliver Slint
/// events, because winit keeps its handler borrowed for that event.
fn request_native_quit(quit: Box<HeldQuit>) {
    #[expect(
        unsafe_code,
        reason = "libdispatch has no binding among the dependencies"
    )]
    // SAFETY: the main queue is a static libSystem object; ownership of the boxed context
    // passes to `terminate_on_main`, which libdispatch calls exactly once on the main thread.
    unsafe {
        dispatch_async_f(
            &raw const _dispatch_main_q,
            Box::into_raw(quit).cast(),
            terminate_on_main,
        );
    }
}

#[expect(
    clippy::expect_used,
    reason = "A native dispatch failure must fail this standalone scenario"
)]
extern "C" fn terminate_on_main(context: *mut c_void) {
    #[expect(unsafe_code, reason = "Reclaims the context leaked by request_native_quit")]
    // SAFETY: `request_native_quit` passed a `Box<HeldQuit>` and libdispatch calls this once.
    let quit = unsafe { Box::from_raw(context.cast::<HeldQuit>()) };
    let main_thread = MainThreadMarker::new().expect("main queue runs on the main thread");
    let native = NSApplication::sharedApplication(main_thread);
    assert!(
        native.isRunning(),
        "termination is requested during the native event loop"
    );
    check_held_termination(*quit);
    native.terminate(None);
}

/// Queued before `terminate:`; runs inside `AppKit`'s nested run loop while it is held.
#[expect(
    clippy::expect_used,
    reason = "A native dispatch failure must fail this standalone scenario"
)]
fn check_held_termination(quit: HeldQuit) {
    let HeldQuit {
        held,
        printer,
        fake,
        job,
    } = quit;
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
            io::stdout()
                .lock()
                .write_all(HELD)
                .expect("report the held termination");
            held.release();
            Ok(())
        });
    })
    .expect("queue the held-termination checks");
}
