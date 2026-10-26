use std::{error::Error, ffi::OsString, fmt, fs, path::Path, sync::Arc};

use agent::{Error as AgentError, settings::Settings, status::Snapshot, store::StateDir};
use fiscal::device::Vendor;
use serde_json::json;

use super::{Command, Failure, execute, parse};

type TestResult = Result<(), Box<dyn Error>>;

// Only tests need Failure to support `?`; production renders a Response.
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
fn manual_credentials_and_unknown_arguments_are_rejected_without_echoing_them() {
    for args in [
        vec!["--token", "secret-canary"],
        vec!["config", "set", "--token", "secret-canary"],
        vec!["pair"],
        vec!["-psn_0_1234567", "status"],
        vec![
            "config",
            "set",
            "--api-url",
            "https://first.example",
            "--api-url",
            "https://second.example",
        ],
        vec![
            "config",
            "set",
            "--diagnostics",
            "on",
            "--diagnostics",
            "off",
        ],
    ] {
        let error = parse(args.into_iter().map(OsString::from)).expect_err("invalid command");
        assert_eq!(error.code, "arguments");
        assert_eq!(error.exit, 2);
        assert_eq!(
            error.message,
            "Nieprawidłowe argumenty. Użyj workshop --help."
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_lone_process_serial_number_opens_the_gui_on_macos() {
    assert_eq!(super::run([OsString::from("-psn_0_1234567")]), None);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn a_process_serial_number_is_an_invalid_argument_outside_macos() {
    use std::process::ExitCode;

    assert_eq!(
        super::run([OsString::from("-psn_0_1234567")]),
        Some(ExitCode::from(2)),
        "a GUI launch could start fiscal processing"
    );
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
        vendor: Some(Vendor::Elzab),
        ..Settings::default()
    };
    settings.save(&dir)?;
    let response = execute(Command::ConfigShow, dir.path())?;
    assert_eq!(
        response.value,
        json!({"ok":true,"config":{
            "apiUrl":"https://example.test","paired":true,"port":"auto",
            "baudRate":9600,"vendor":"elzab","telemetryEnabled":true,"startOnLaunch":false
        }})
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; setup propagates I/O"
)]
fn settings_changes_refuse_the_running_apps_lock_and_preserve_config() -> TestResult {
    let temp = tempfile::tempdir()?;
    let dir = Arc::new(StateDir::open_application(temp.path().join("state"))?);
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
    let settings = Settings {
        api_url: "https://paired.example.test".into(),
        token: "synthetic-existing-credential".into(),
        ..Settings::default()
    };
    settings.save(&StateDir::open(path.clone())?)?;
    let before = fs::read(path.join("settings.json"))?;
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
    assert_eq!((error.exit, error.code), (3, "configuration"));
    assert_eq!(
        error.message,
        concat!(
            "Adres API wymaga https (http tylko dla localhost), ",
            "bez danych logowania, zapytania i fragmentu."
        )
    );
    assert_eq!(fs::read(path.join("settings.json"))?, before);
    assert_eq!(Settings::load_at(&path)?, Some(settings));
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
    let dir = StateDir::open_application(temp.path().join("state"))?;
    Settings {
        api_url: "https://private.example.test".into(),
        token: "token-canary".into(),
        ..Settings::default()
    }
    .save(&dir)?;
    let snapshot = Snapshot {
        processing: true,
        connected: true,
        vendor: Some(Vendor::Novitus),
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
            "processing":true,"connected":true,"uniqueNumber":"DEM0000000001","vendor":"novitus",
            "printerId":"00000000-0000-0000-0000-000000000009",
            "blocked":true,"reportUnresolved":false,"orphanedReceipt":true,
            "updatedAt":"2026-10-24T12:34:56Z"
        }})
    );
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
fn a_cli_command_holding_the_state_lock_is_not_a_running_app() -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("state");
    // A crashed GUI session left a processing snapshot behind.
    Snapshot {
        processing: true,
        connected: true,
        vendor: None,
        unique_number: Some("DEM0000000001".into()),
        printer_id: None,
        blocked: false,
        report_unresolved: false,
        orphaned_receipt: false,
        updated_at: "2026-10-24T12:34:56Z".parse()?,
    }
    .save(&StateDir::open(path.clone())?)?;
    // `pair`, `config set` and `autostart` hold the state lock while they run.
    let _command = StateDir::open(path.clone())?;
    let status = execute(Command::Status, &path)?;
    assert_eq!(status.value, json!({"ok":true,"appRunning":false}));
    let check = execute(Command::Check, &path)?;
    assert_eq!(check.value["appRunning"], json!(false));
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
    let dir = StateDir::open_application(temp.path().join("state"))?;
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
        let stdout = String::from_utf8(output.stdout)?;
        let responses = stdout
            .lines()
            .filter(|line| line.starts_with('{'))
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            responses,
            [json!({"ok":true, "autostart":value == "on", "startOnLaunch":value == "on"})]
        );
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
    for (status, code, expected_exit, expected_code, expected_message) in [
        (
            400,
            "fiscal_pairing_invalid",
            3,
            "pairing_invalid",
            "Nieprawidłowy, wykorzystany lub wygasły kod parowania. Wygeneruj nowy kod w Holuto.",
        ),
        (
            429,
            "rate_limited",
            7,
            "rate_limited",
            "Zbyt wiele prób parowania. Spróbuj później.",
        ),
        (
            401,
            "unauthorized",
            6,
            "unauthorized",
            "Holuto odrzuciło parowanie drukarki (401). Sparuj drukarkę ponownie w Ustawieniach.",
        ),
        (
            503,
            "unavailable",
            5,
            "holuto",
            "Holuto odpowiedziało błędem 503",
        ),
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
        assert_eq!(error.message, expected_message);
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

#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Child dispatch checks the public CLI output, exit code and persisted configuration"
)]
#[expect(
    clippy::too_many_lines,
    reason = "Keep literal CLI cases beside their subprocess and output assertions"
)]
fn json_dispatch_round_trips_configuration_and_reports_exact_errors() -> TestResult {
    use std::{
        env,
        process::{Command as Process, ExitCode},
    };
    const CHILD: &str = "WORKSHOP_CLI_DISPATCH_CHILD";
    if let Some(arguments) = env::var_os(CHILD) {
        let arguments: Vec<String> = serde_json::from_str(&arguments.to_string_lossy())?;
        let expected: u8 = env::var("WORKSHOP_CLI_EXIT")?.parse()?;
        assert_eq!(
            super::run(arguments.into_iter().map(OsString::from)),
            Some(ExitCode::from(expected))
        );
        fs::write(
            env::var_os("WORKSHOP_CLI_OUTCOME").ok_or("missing outcome path")?,
            "dispatched\n",
        )?;
        return Ok(());
    }
    let temporary = tempfile::tempdir()?;
    let invalid = json!({"ok":false,"error":{
        "code":"arguments","message":"Nieprawidłowe argumenty. Użyj workshop --help."
    }});
    for (arguments, exit, expected) in [
        (
            vec!["check"],
            3,
            json!({"ok":false,"configured":false,"appRunning":false,
            "errors":["Wpisz adres API w Ustawieniach."]}),
        ),
        (
            vec!["config", "show"],
            0,
            json!({"ok":true,"config":{
                "apiUrl":"","paired":false,"port":"auto","baudRate":9600,"vendor":null,
                "telemetryEnabled":true,"startOnLaunch":false
            }}),
        ),
        (
            vec!["config", "set", "--speed", "19200", "--speed", "9600"],
            2,
            invalid.clone(),
        ),
        (vec!["config", "set"], 2, invalid.clone()),
        (vec!["config", "set", "--diagnostics", "maybe"], 2, invalid),
        (
            vec![
                "config",
                "set",
                "--api-url",
                "https://example.test/api",
                "--port",
                "TEST-NOT-SERIAL",
                "--speed",
                "19200",
                "--diagnostics",
                "off",
            ],
            0,
            json!({"ok":true}),
        ),
        (
            vec!["config", "show"],
            0,
            json!({"ok":true,"config":{
                "apiUrl":"https://example.test/api","paired":false,"port":"TEST-NOT-SERIAL",
                "baudRate":19200,"vendor":null,"telemetryEnabled":false,"startOnLaunch":false
            }}),
        ),
        (
            vec!["check"],
            3,
            json!({"ok":false,"configured":false,"appRunning":false,
            "errors":["Sparuj drukarkę kodem z Holuto w Ustawieniach."]}),
        ),
        (vec!["status"], 0, json!({"ok":true,"appRunning":false})),
    ] {
        let mut arguments = arguments;
        arguments.push("--json");
        let outcome = temporary.path().join("outcome");
        #[expect(
            clippy::disallowed_methods,
            reason = "CLI path overrides are child-local"
        )]
        let output = Process::new(env::current_exe()?)
            .args([
                "--exact",
                "cli::tests::json_dispatch_round_trips_configuration_and_reports_exact_errors",
                "--nocapture",
            ])
            .env(CHILD, serde_json::to_string(&arguments)?)
            .env("WORKSHOP_CLI_EXIT", exit.to_string())
            .env("WORKSHOP_CLI_OUTCOME", &outcome)
            .env("XDG_STATE_HOME", temporary.path())
            .output()?;
        assert!(output.status.success(), "{arguments:?}: {output:?}");
        assert_eq!(fs::read_to_string(&outcome)?, "dispatched\n");
        fs::remove_file(&outcome)?;
        assert!(output.stderr.is_empty(), "{output:?}");
        let stdout = String::from_utf8(output.stdout)?;
        assert_eq!(
            stdout
                .lines()
                .filter(|line| line.starts_with('{'))
                .map(serde_json::from_str::<serde_json::Value>)
                .collect::<Result<Vec<_>, _>>()?,
            [expected],
            "{arguments:?}"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Real CLI dispatch selects its public output stream"
)]
fn argument_errors_are_polish_stderr_unless_json_is_requested() -> TestResult {
    use std::{
        env,
        process::{Command as Process, ExitCode},
    };
    const CHILD: &str = "WORKSHOP_CLI_STREAM_CHILD";
    if let Some(mode) = env::var_os(CHILD) {
        let mut arguments = vec![OsString::from("--unknown")];
        if mode == "json" {
            arguments.push("--json".into());
        }
        assert_eq!(super::run(arguments), Some(ExitCode::from(2)));
        return Ok(());
    }
    for mode in ["human", "json"] {
        #[expect(
            clippy::disallowed_methods,
            reason = "Capture actual CLI streams in a child"
        )]
        let output = Process::new(env::current_exe()?)
            .args([
                "--exact",
                "cli::tests::argument_errors_are_polish_stderr_unless_json_is_requested",
                "--nocapture",
            ])
            .env(CHILD, mode)
            .output()?;
        assert!(output.status.success(), "{output:?}");
        if mode == "human" {
            assert_eq!(
                String::from_utf8(output.stderr)?,
                "Nieprawidłowe argumenty. Użyj workshop --help.\n"
            );
        } else {
            assert!(output.stderr.is_empty());
            let stdout = String::from_utf8(output.stdout)?;
            assert_eq!(
                stdout
                    .lines()
                    .filter(|line| line.starts_with('{'))
                    .collect::<Vec<_>>(),
                [concat!(
                    r#"{"error":{"code":"arguments","message":"#,
                    r#""Nieprawidłowe argumenty. Użyj workshop --help."},"ok":false}"#
                )]
            );
        }
    }
    Ok(())
}
