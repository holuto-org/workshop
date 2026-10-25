use std::{env, path::PathBuf, process::Command};

use super::*;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "UI assertions propagate fallible fixture and child-process setup"
)]
fn login_checkbox_registers_and_persists_without_pairing() -> Result<(), Box<dyn Error>> {
    const CHILD: &str = "WORKSHOP_AUTOSTART_UI_CHILD";
    if env::var_os(CHILD).is_none() {
        let home = TempDir::new()?;
        #[expect(
            clippy::disallowed_methods,
            reason = "The child isolates XDG autostart registration from the operator's session"
        )]
        let output = Command::new(env::current_exe()?)
            .args([
                "--exact",
                concat!(
                    "application::tests::autostart::",
                    "login_checkbox_registers_and_persists_without_pairing"
                ),
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .output()?;
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(" 1 passed"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let (_temporary, app, window) = fixture_with_event_loop()?;
    control(
        &window,
        "Uruchamiaj przy logowaniu",
        AccessibleRole::Checkbox,
    )?
    .invoke_accessible_default_action();
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    let saved = Settings::load(&app.borrow().dir)?.ok_or("missing preferences")?;
    assert!(saved.start_on_launch);
    assert_eq!(saved.api_url, "");
    assert_eq!(saved.token.as_str(), "");
    assert!(window.get_start_on_launch());
    let path = PathBuf::from(env::var_os("XDG_CONFIG_HOME").ok_or("missing test config")?)
        .join("autostart/workshop.desktop");
    assert!(
        path.is_file(),
        "checkbox registers a real per-user login entry"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}
