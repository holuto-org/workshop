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
        #[expect(
            clippy::disallowed_methods,
            reason = "Only a child process may receive a real SIGTERM"
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
            .env(CHILD, "1")
            .output()?;
        assert!(
            output.status.success(),
            "the child must exit normally after SIGTERM, got {:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let (_temporary, app, _window) = fixture_with_event_loop()?;
    #[expect(
        clippy::disallowed_methods,
        reason = "Delivers the session-end signal a logout or shutdown sends"
    )]
    let sent = Command::new("kill")
        .args(["-TERM", &process::id().to_string()])
        .status()?;
    assert!(sent.success(), "kill delivered SIGTERM");
    assert!(
        quit_before_deadline()?,
        "SIGTERM routes through the cooperative quit path"
    );
    assert!(app.borrow().quitting, "the signal requested quit");
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}
