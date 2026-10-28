use std::{env, path::PathBuf, process::Command};

use super::*;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "UI assertions propagate fallible fixture and child-process setup"
)]
fn login_checkbox_registers_and_persists_without_pairing_or_unsaved_edits()
-> Result<(), Box<dyn Error>> {
    const CHILD: &str = "WORKSHOP_AUTOSTART_UI_CHILD";
    if env::var_os(CHILD).is_none() {
        let home = TempDir::new()?;
        let outcome = home.path().join("outcome");
        #[expect(
            clippy::disallowed_methods,
            reason = "The child isolates XDG autostart registration from the operator's session"
        )]
        let output = Command::new(env::current_exe()?)
            .args([
                "--exact",
                concat!(
                    "application::tests::autostart::",
                    "login_checkbox_registers_and_persists_without_pairing_or_unsaved_edits"
                ),
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("WORKSHOP_AUTOSTART_UI_OUTCOME", &outcome)
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            fs::read_to_string(outcome)?,
            "registered; persisted; stopped\n"
        );
        return Ok(());
    }
    let (_temporary, app, window) = fixture_with_event_loop()?;
    window.set_tab(2);
    // Not committed yet: only committing a field, Uruchom or pairing persists connection edits.
    window.set_port("/dev/ttyUNSAVED".into());
    // An opt-out whose own write has not landed yet must still reach the new file.
    window.set_telemetry_enabled(false);
    control(
        &window,
        "Uruchamiaj przy logowaniu",
        AccessibleRole::Checkbox,
    )?
    .invoke_accessible_default_action();
    assert!(app.borrow().model.saving);
    let pending = app.borrow().writes_pending.len();
    control(
        &window,
        "Uruchamiaj przy logowaniu",
        AccessibleRole::Checkbox,
    )?
    .invoke_accessible_default_action();
    assert_eq!(
        app.borrow().writes_pending.len(),
        pending,
        "saving refuses a second registration"
    );
    app.borrow_mut().stop(true)?;
    slint::run_event_loop_until_quit()?;
    let saved = Settings::load(&app.borrow().dir)?.ok_or("missing preferences")?;
    assert_eq!(
        saved,
        Settings {
            start_on_launch: true,
            telemetry_enabled: false,
            ..Settings::default()
        }
    );
    assert!(window.get_start_on_launch());
    let path = PathBuf::from(env::var_os("XDG_CONFIG_HOME").ok_or("missing test config")?)
        .join("autostart/workshop.desktop");
    assert!(
        path.is_file(),
        "checkbox registers a real per-user login entry"
    );
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    fs::write(
        env::var_os("WORKSHOP_AUTOSTART_UI_OUTCOME").ok_or("missing outcome path")?,
        "registered; persisted; stopped\n",
    )?;
    Ok(())
}
