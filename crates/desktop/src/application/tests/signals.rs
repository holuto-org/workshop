use std::{
    env,
    process::{self, Command},
};

use super::*;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions propagate fallible fixture and child-process setup"
)]
fn session_end_signal_quits_cooperatively_instead_of_killing_the_app() -> Result<(), Box<dyn Error>>
{
    const CHILD: &str = "WORKSHOP_SIGNAL_CHILD";
    if env::var_os(CHILD).is_none() {
        let temporary = TempDir::new()?;
        for signal in ["TERM", "HUP", "INT"] {
            let outcome = temporary.path().join(signal);
            #[expect(
                clippy::disallowed_methods,
                reason = "Only isolated children receive real session-end signals"
            )]
            let output = Command::new(env::current_exe()?)
                .args([
                    "--exact",
                    concat!(
                        "application::tests::signals::",
                        "session_end_signal_quits_cooperatively_instead_of_killing_the_app"
                    ),
                    "--nocapture",
                ])
                .env(CHILD, signal)
                .env("WORKSHOP_SIGNAL_OUTCOME", &outcome)
                .output()?;
            assert!(output.status.success(), "{signal}: {output:?}");
            assert_eq!(fs::read_to_string(outcome)?, "quitting; no worker\n");
        }
        return Ok(());
    }
    let (_temporary, app, _window) = fixture_with_event_loop()?;
    let signal = env::var(CHILD)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "Delivers the session-end signal a logout or shutdown sends"
    )]
    let sent = Command::new("kill")
        .args([&format!("-{signal}"), &process::id().to_string()])
        .status()?;
    assert!(sent.success(), "kill delivered SIGTERM");
    assert!(
        quit_before_deadline()?,
        "SIGTERM routes through the cooperative quit path"
    );
    assert!(app.borrow().quitting, "the signal requested quit");
    assert!(app.borrow().worker.is_none());
    fs::write(
        env::var_os("WORKSHOP_SIGNAL_OUTCOME").ok_or("missing outcome path")?,
        "quitting; no worker\n",
    )?;
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}
