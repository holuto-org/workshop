//! Native `AppKit` regression: a harness-free target keeps Rust's actual main thread.
//!
//! This is the deliberate exception to the Slint testing backend: only a real winit
//! window can prove `AppKit` dispatches its termination selector to our installed hook.
//! Production source is reused; the scenario is a child of `Application`
//! so no second controller or test-only production API is needed.
//!
//! The small listing/exact protocol follows nextest's custom-harness contract:
//! <https://nexte.st/docs/design/custom-test-harnesses/>.

#[cfg(target_os = "macos")]
use std::{
    env,
    error::Error,
    io::{self, Write},
    process::Command,
};

#[cfg(target_os = "macos")]
#[expect(
    dead_code,
    reason = "The native scenario reuses production modules but not every startup path"
)]
mod application {
    include!("../application.rs");

    pub(crate) mod native_quit_test {
        include!("../application/macos/tests.rs");
    }
}
#[cfg(target_os = "macos")]
#[path = "../autostart.rs"]
mod autostart;
#[cfg(target_os = "macos")]
#[path = "../model.rs"]
mod model;
#[cfg(target_os = "macos")]
#[expect(
    dead_code,
    reason = "The native scenario never initializes external telemetry"
)]
#[path = "../telemetry.rs"]
mod telemetry;
#[cfg(target_os = "macos")]
#[path = "../tray.rs"]
mod tray;
#[cfg(target_os = "macos")]
#[expect(
    clippy::pub_use,
    clippy::impl_trait_in_params,
    clippy::multiple_inherent_impl,
    clippy::same_name_method,
    reason = "Slint 1.18 emits these items; handwritten Rust keeps workspace lints"
)]
mod ui {
    slint::slint! {
        #[style = "fluent"]
        export * from "../../ui/app.slint";
    }
}

#[cfg(target_os = "macos")]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Native test assertions propagate process and fixture errors"
)]
fn main() -> Result<(), Box<dyn Error>> {
    const COMPLETED: &[u8] = b"native quit completed after stopped\n";
    const TEST_NAME: &str = "native_quit_waits_for_worker";
    if env::args().any(|argument| argument == "--list") {
        if !env::args().any(|argument| argument == "--ignored") {
            writeln!(io::stdout().lock(), "{TEST_NAME}: test")?;
        }
        return Ok(());
    }
    if env::args().any(|argument| argument == "--ignored")
        || (env::args().any(|argument| argument == "--exact")
            && !env::args().any(|argument| argument == TEST_NAME))
    {
        return Ok(());
    }
    if env::var_os("WORKSHOP_NATIVE_QUIT_CHILD").is_some() {
        application::native_quit_test::run()?;
        io::stdout().lock().write_all(COMPLETED)?;
        return Ok(());
    }
    // AppKit's erroneous immediate exit(0) must not look like a successful test.
    #[expect(
        clippy::disallowed_methods,
        reason = "The child detects AppKit termination before its final assertions"
    )]
    let child = Command::new(env::current_exe()?)
        .env("WORKSHOP_NATIVE_QUIT_CHILD", "1")
        .output()?;
    assert!(
        child.status.success() && child.stdout.ends_with(COMPLETED),
        "native quit did not reach the post-Stopped assertions: status={}, stderr={}",
        child.status,
        String::from_utf8_lossy(&child.stderr)
    );
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn main() {}
