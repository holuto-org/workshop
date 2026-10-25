use std::{error::Error, ffi::OsString, fmt, fs, path::Path, sync::Arc};

use agent::{Error as AgentError, settings::Settings, status::Snapshot, store::StateDir};
use serde_json::json;

use super::{Command, Failure, execute, parse};

type TestResult = Result<(), Box<dyn Error>>;

// Lets `?` box CLI failures; production renders them as a `Response` instead.
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl Error for Failure {}

#[test]
fn ports_json_includes_brand_hints_while_human_output_keeps_names_only() {
    use fiscal::{device::Vendor, link::Candidate};

    let response = super::ports_response(&[
        Candidate {
            port: "/dev/ttyACM0".into(),
            hint: Some(Vendor::Posnet),
        },
        Candidate {
            port: "/dev/ttyACM1".into(),
            hint: Some(Vendor::Novitus),
        },
        Candidate {
            port: "/dev/ttyACM2".into(),
            hint: Some(Vendor::Elzab),
        },
        Candidate {
            port: "/dev/ttyUSB0".into(),
            hint: None,
        },
    ]);
    assert_eq!(
        response.value,
        json!({
            "ok": true,
            "ports": [
                {"port": "/dev/ttyACM0", "hint": "posnet"},
                {"port": "/dev/ttyACM1", "hint": "novitus"},
                {"port": "/dev/ttyACM2", "hint": "elzab"},
                {"port": "/dev/ttyUSB0", "hint": null}
            ]
        })
    );
    assert_eq!(
        response.human,
        "/dev/ttyACM0\n/dev/ttyACM1\n/dev/ttyACM2\n/dev/ttyUSB0"
    );
    assert_eq!(response.exit, 0);
}

#[test]
fn empty_ports_response_has_no_human_output() {
    let response = super::ports_response(&[]);
    assert_eq!(response.value, json!({"ok": true, "ports": []}));
    assert_eq!(response.human, "");
    assert_eq!(response.exit, 0);
}

fn save_api_url(path: &Path, api_url: &str) -> Result<(), AgentError> {
    let dir = StateDir::open(path.to_owned())?;
    Settings {
        api_url: api_url.into(),
        ..Settings::default()
    }
    .save(&dir)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove both CLI JSON vendor contracts."
)]
fn config_show_and_status_report_vendor() -> TestResult {
    use fiscal::device::Vendor;

    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    Settings {
        vendor: Some(Vendor::Elzab),
        ..Settings::default()
    }
    .save(&dir)?;
    assert_eq!(
        execute(Command::ConfigShow, dir.path())?.value["config"]["vendor"],
        "elzab"
    );
    Snapshot {
        processing: true,
        connected: true,
        vendor: Some(Vendor::Novitus),
        unique_number: Some("SAME-SERIAL".into()),
        printer_id: None,
        blocked: true,
        report_unresolved: true,
        orphaned_receipt: false,
        updated_at: jiff::Timestamp::UNIX_EPOCH,
    }
    .save(&dir)?;
    assert_eq!(
        execute(Command::Status, dir.path())?.value["state"]["vendor"],
        "novitus"
    );
    Ok(())
}

#[test]
fn manual_credentials_and_unknown_arguments_are_rejected_without_echoing_them() {
    for args in [
        vec!["--token", "secret-canary"],
        vec!["config", "set", "--token", "secret-canary"],
        vec!["pair"],
    ] {
        let error = parse(args.into_iter().map(OsString::from)).expect_err("invalid command");
        assert_eq!(error.code, "arguments");
        assert_eq!(error.exit, 2);
        assert!(!error.message.contains("secret-canary"));
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn json_is_accepted_before_or_after_every_command() -> TestResult {
    for (words, expected) in [
        (vec!["ports"], Command::Ports),
        (vec!["check"], Command::Check),
        (vec!["status"], Command::Status),
        (vec!["config", "show"], Command::ConfigShow),
        (vec!["pair", "ABCD1234"], Command::Pair("ABCD1234".into())),
        (vec!["autostart", "off"], Command::Autostart(false)),
        (vec!["--help"], Command::Help),
        (vec!["--version"], Command::Version),
        (
            vec!["config", "set", "--speed", "9600"],
            Command::ConfigSet(super::Changes {
                speed: Some(9600),
                ..super::Changes::default()
            }),
        ),
    ] {
        for before in [true, false] {
            let mut args = words.iter().map(OsString::from).collect::<Vec<_>>();
            if before {
                args.insert(0, "--json".into());
            } else {
                args.push("--json".into());
            }
            let parsed = parse(args)?;
            assert!(parsed.json);
            assert_eq!(parsed.command, expected);
        }
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn configuration_output_redacts_saved_credentials() -> TestResult {
    let temp = tempfile::tempdir()?;
    let dir = StateDir::open(temp.path().join("state"))?;
    let settings = Settings {
        api_url: "https://example.test".into(),
        token: "secret-canary".into(),
        ..Settings::default()
    };
    settings.save(&dir)?;
    let response = execute(Command::ConfigShow, dir.path())?;
    assert_eq!(
        response.value,
        json!({"ok":true,"config":{
            "apiUrl":"https://example.test","paired":true,"port":"auto",
            "baudRate":9600,"vendor":null,"telemetryEnabled":true,"startOnLaunch":false
        }})
    );
    assert!(!response.value.to_string().contains("secret-canary"));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn settings_changes_refuse_the_running_apps_lock_and_preserve_config() -> TestResult {
    let temp = tempfile::tempdir()?;
    let dir = Arc::new(StateDir::open(temp.path().join("state"))?);
    let settings = Settings {
        api_url: "https://example.test".into(),
        token: "secret-canary".into(),
        ..Settings::default()
    };
    settings.save(&dir)?;
    for args in [
        vec!["config", "set", "--speed", "19200"],
        vec!["pair", "ABCD1234"],
        vec!["autostart", "on"],
    ] {
        let parsed = parse(args.into_iter().map(OsString::from))?;
        let error = execute(parsed.command, dir.path()).expect_err("app lock held");
        assert_eq!((error.exit, error.code), (4, "app_running"));
        assert_eq!(Settings::load(&dir)?, Some(settings.clone()));
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn fresh_installation_check_reports_missing_configuration_without_opening_ports() -> TestResult {
    let temp = tempfile::tempdir()?;
    let response = execute(Command::Check, &temp.path().join("state"))?;
    assert_eq!(response.exit, 3);
    assert_eq!(
        response.value,
        json!({
            "ok":false,"configured":false,"appRunning":false,
            "errors":["Wpisz adres API w Ustawieniach."]
        })
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn invalid_configuration_is_not_partially_saved() -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("state");
    let parsed = parse(
        [
            "config",
            "set",
            "--api-url",
            "http://unsafe.example",
            "--speed",
            "19200",
        ]
        .map(OsString::from),
    )?;
    let error = execute(parsed.command, &path).expect_err("unsafe URL");
    assert_eq!(error.exit, 3);
    assert!(!path.join("settings.json").exists());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn changing_the_api_address_forgets_the_token_paired_with_the_previous_one() -> TestResult {
    for (api_url, token) in [
        ("https://example.test/", "secret-canary"),
        ("https://other.example.test", ""),
    ] {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("state");
        Settings {
            api_url: "https://example.test".into(),
            token: "secret-canary".into(),
            ..Settings::default()
        }
        .save(&StateDir::open(path.clone())?)?;
        let parsed = parse(["config", "set", "--api-url", api_url].map(OsString::from))?;
        assert_eq!(execute(parsed.command, &path)?.exit, 0);
        let saved = Settings::load_at(&path)?.ok_or("missing settings")?;
        assert_eq!(
            (saved.api_url.as_str(), saved.token.as_str()),
            (api_url, token),
            "a token is kept only for the address it was paired with"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn status_exposes_the_running_apps_published_state_without_configuration_secrets() -> TestResult {
    let temp = tempfile::tempdir()?;
    let dir = StateDir::open(temp.path().join("state"))?;
    Settings {
        api_url: "https://private.example.test".into(),
        token: "token-canary".into(),
        ..Settings::default()
    }
    .save(&dir)?;
    let snapshot = Snapshot {
        processing: true,
        connected: true,
        vendor: None,
        unique_number: Some("DEM0000000001".into()),
        printer_id: Some("00000000-0000-0000-0000-000000000009".into()),
        blocked: true,
        report_unresolved: false,
        orphaned_receipt: true,
        updated_at: "2026-10-24T12:34:56Z".parse()?,
    };
    snapshot.save(&dir)?;
    let response = execute(Command::Status, dir.path())?;
    assert_eq!(
        response.value,
        json!({"ok":true,"appRunning":true,"state":{
            "processing":true,"connected":true,"uniqueNumber":"DEM0000000001","vendor":null,
            "printerId":"00000000-0000-0000-0000-000000000009",
            "blocked":true,"reportUnresolved":false,"orphanedReceipt":true,
            "updatedAt":"2026-10-24T12:34:56Z"
        }})
    );
    assert!(!response.value.to_string().contains("token-canary"));
    assert!(!response.value.to_string().contains("private.example.test"));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove status; setup propagates I/O"
)]
fn unlocked_status_ignores_even_a_corrupt_snapshot_left_after_a_crash() -> TestResult {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("status.json"), b"corrupt secret-canary")?;
    let response = execute(Command::Status, temp.path())?;
    assert_eq!(response.value, json!({"ok":true,"appRunning":false}));
    assert_eq!(response.exit, 0);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove status; setup propagates I/O"
)]
fn running_app_without_a_published_snapshot_reports_unavailable_instead_of_guessing() -> TestResult
{
    let temp = tempfile::tempdir()?;
    let dir = StateDir::open(temp.path().join("state"))?;
    let error = execute(Command::Status, dir.path()).expect_err("not yet published");
    assert_eq!((error.exit, error.code), (1, "status_unavailable"));
    assert_eq!(
        error.message,
        "Aplikacja nie udostępniła jeszcze stanu. Spróbuj ponownie."
    );
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn autostart_command_registers_login_and_persists_processing_preference() -> TestResult {
    use std::{
        env, fs,
        process::{Command as Process, ExitCode},
    };
    if let Some(value) = env::var_os("WORKSHOP_AUTOSTART_CHILD") {
        assert_eq!(
            super::run(["autostart".into(), value, "--json".into()]),
            Some(ExitCode::SUCCESS)
        );
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let executable = env::current_exe()?;
    for value in ["on", "off"] {
        #[expect(
            clippy::disallowed_methods,
            reason = "Isolate login registration in a child HOME"
        )]
        let output = Process::new(&executable)
            .args([
                "--exact",
                "cli::tests::autostart_command_registers_login_and_persists_processing_preference",
                "--nocapture",
            ])
            .env("WORKSHOP_AUTOSTART_CHILD", value)
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env("XDG_STATE_HOME", temp.path().join("state"))
            .output()?;
        assert!(output.status.success(), "child: {output:?}");
        let state = temp
            .path()
            .join("state")
            .join(if cfg!(feature = "simulator") {
                "workshop-simulator"
            } else {
                "workshop"
            });
        let settings = Settings::load_at(&state)?.ok_or("settings were not written")?;
        assert_eq!(settings.start_on_launch, value == "on");
        let artifact = temp.path().join("config/autostart/workshop.desktop");
        if value == "on" {
            assert_eq!(
                fs::read_to_string(&artifact)?,
                format!(
                    "[Desktop Entry]\nType=Application\nName=Holuto Workshop\n\
                     Exec=\"{}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
                    executable.display()
                )
            );
        } else {
            assert!(!artifact.exists());
        }
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn pairing_command_persists_credentials_and_returns_only_printer_identity() -> TestResult {
    use agent::testing::{Holuto, PRINTER_ID};
    let server = Holuto::new();
    server.pairing_code("ABCD1234");
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("state");
    save_api_url(&path, server.url())?;
    let response = execute(Command::Pair("abcd-1234".into()), &path)?;
    assert_eq!(response.exit, 0);
    assert_eq!(response.value, json!({"ok":true,"printerId":PRINTER_ID}));
    let saved = Settings::load_at(&path)?.ok_or("missing credentials")?;
    assert_eq!(saved.token.as_str(), format!("{:0>42}A", 1));
    assert!(!response.value.to_string().contains(saved.token.as_str()));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn pairing_command_maps_refusals_to_stable_exit_codes_without_saving() -> TestResult {
    use agent::testing::{Fault, Holuto};
    for (status, code, expected_exit, expected_code) in [
        (400, "fiscal_pairing_invalid", 3, "pairing_invalid"),
        (429, "rate_limited", 7, "rate_limited"),
        (401, "unauthorized", 6, "unauthorized"),
        (503, "unavailable", 5, "holuto"),
    ] {
        let server = Holuto::new();
        server.pairing_code("ABCD1234");
        server.fault(
            "POST",
            "/agent/pair",
            Fault::Problem {
                status,
                code: code.into(),
                detail: "secret-canary".into(),
            },
        );
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("state");
        save_api_url(&path, server.url())?;
        let before = Settings::load_at(&path)?;
        let error = execute(Command::Pair("ABCD1234".into()), &path).expect_err("pairing refusal");
        assert_eq!((error.exit, error.code), (expected_exit, expected_code));
        assert!(!error.message.contains("secret-canary"));
        assert_eq!(Settings::load_at(&path)?, before);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove pairing; setup propagates I/O"
)]
fn malformed_pairing_code_exits_three_without_a_backend_request_or_settings_change() -> TestResult {
    use agent::testing::Holuto;
    let server = Holuto::new();
    let temp = tempfile::tempdir()?;
    save_api_url(temp.path(), server.url())?;
    let before = Settings::load_at(temp.path())?;
    let error = execute(Command::Pair("bad".into()), temp.path()).expect_err("malformed proof");
    assert_eq!((error.exit, error.code), (3, "pairing_invalid"));
    assert_eq!(
        error.message,
        "Kod parowania ma 8 znaków z Holuto, na przykład ABCD-1234."
    );
    assert_eq!(server.requests(), []);
    assert_eq!(Settings::load_at(temp.path())?, before);
    Ok(())
}
