//! Real `AppKit` dispatch requires a winit window on Rust's main thread.
//! Reuse production modules and nest the scenario under `Application` to avoid a test-only API.
//!
//! The harness implements nextest's listing/exact protocol:
//! <https://nexte.st/docs/design/custom-test-harnesses/>.

#[cfg(target_os = "macos")]
use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
    path::Path,
    process::Command,
};

#[cfg(target_os = "macos")]
use agent::{status::Snapshot, store::StateDir, testing::JOB_ID, worker};
#[cfg(target_os = "macos")]
use serde_json::json;

// Cargo builds a `harness = false` target with `--cfg test` but without `--test`, so each
// included module's `#[cfg(test)] mod tests` compiles with its `#[test]` functions stripped,
// leaving their imports and helpers unused. The binary's own test target lints them in full.
#[cfg(target_os = "macos")]
#[expect(
    dead_code,
    reason = "The native scenario reuses production modules but not every startup path"
)]
#[expect(
    unused_imports,
    clippy::wildcard_imports,
    reason = "Included unit tests are stripped in this harness"
)]
mod application {
    include!("../application.rs");

    pub(crate) mod native_quit_test {
        include!("../application/macos/tests.rs");
    }
}
#[cfg(target_os = "macos")]
#[expect(
    dead_code,
    unused_imports,
    reason = "Included unit tests are stripped in this harness"
)]
#[path = "../autostart.rs"]
mod autostart;
#[cfg(target_os = "macos")]
#[path = "../localization.rs"]
mod localization;
#[cfg(target_os = "macos")]
#[expect(
    dead_code,
    unused_imports,
    clippy::wildcard_imports,
    reason = "Included unit tests are stripped in this harness"
)]
#[path = "../model.rs"]
mod model;
#[cfg(target_os = "macos")]
#[expect(
    dead_code,
    reason = "The native scenario never initializes external telemetry"
)]
#[expect(
    unused_imports,
    clippy::wildcard_imports,
    reason = "Included unit tests are stripped in this harness"
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
    const TEST_NAME: &str = "native_quit_waits_for_worker";
    const CHILD: &str = "WORKSHOP_NATIVE_QUIT_CHILD";
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
    if let Some(state) = env::var_os(CHILD) {
        return application::native_quit_test::run(Path::new(&state));
    }
    let state = tempfile::tempdir()?;
    #[expect(
        clippy::disallowed_methods,
        reason = "AppKit ends the child process; only a parent can judge that exit"
    )]
    let child = Command::new(env::current_exe()?)
        .env(CHILD, state.path())
        .output()?;
    assert!(
        child.status.success() && child.stdout.ends_with(application::native_quit_test::HELD),
        "AppKit did not hold the termination until it exited: status={}, stderr={}",
        child.status,
        String::from_utf8_lossy(&child.stderr)
    );
    // An exit before Event::Stopped would leave the receipt unsettled or processing published.
    let published = Snapshot::load_at(state.path())?.ok_or("no published status")?;
    assert!(
        !published.processing && !published.connected,
        "AppKit exited before the stopped status was published"
    );
    let dir = StateDir::open(state.path().to_owned())?;
    let accepted: serde_json::Value =
        serde_json::from_slice(&fs::read(state.path().join("accepted-job.json"))?)?;
    assert_eq!(
        accepted.get("id"),
        Some(&json!(JOB_ID)),
        "the evidence describes the queued job"
    );
    assert_eq!(
        accepted.get("state"),
        Some(&json!("done")),
        "Holuto accepted the result before native exit"
    );
    assert_eq!(
        accepted.get("receiptNumber"),
        Some(&json!("1")),
        "Holuto recorded the printed receipt number"
    );
    assert!(
        !worker::journal_block(&dir).blocked,
        "AppKit exited before the fiscal outcome was settled"
    );
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn main() {}
