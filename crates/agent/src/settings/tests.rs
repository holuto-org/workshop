#[cfg(not(windows))]
use std::{env, process::Command};
use std::{error::Error as StdError, fs, io::ErrorKind};

use super::{Settings, Token};
use crate::{Error, store::StateDir};

type TestResult = Result<(), Box<dyn StdError>>;

fn settings() -> Settings {
    Settings {
        api_url: "https://api.example/v1/".to_owned(),
        token: "secret-token".into(),
        port: "Żółć/drukarka".to_owned(),
        baud_rate: 38_400,
        vendor: None,
        telemetry_enabled: false,
        start_on_launch: false,
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn saving_settings_preserves_the_reviewed_stored_format() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("Zażółć gęślą jaźń"))?;
    settings().save(&dir)?;
    assert_eq!(
        fs::read(dir.file("settings.json"))?,
        include_bytes!("fixtures/settings.json"),
        "the golden fixture pins the current on-disk format"
    );
    assert_eq!(
        Settings::load(&dir)?,
        Some(settings()),
        "the opt-out and Polish path survive restart"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn the_token_is_only_exposed_by_explicit_access_or_storage() -> TestResult {
    let token = Token::from(String::from("do-not-log-this"));
    assert_eq!(
        token.as_str(),
        "do-not-log-this",
        "HTTP authentication can explicitly access the secret"
    );
    assert_eq!(
        format!("{token:?}"),
        "[REDACTED]",
        "Debug never reveals credentials"
    );
    assert!(
        !format!("{:?}", settings()).contains("secret-token"),
        "derived Settings Debug must also redact"
    );
    assert_eq!(
        serde_json::to_string(&token)?,
        "\"do-not-log-this\"",
        "the on-disk token stays a JSON string"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn malformed_or_unknown_settings_are_not_treated_as_unconfigured() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    assert_eq!(
        Settings::load(&dir)?,
        None,
        "only a missing file means unconfigured"
    );
    for bytes in [b"{".as_slice(), b"{\"unexpected\":true}".as_slice()] {
        fs::write(dir.file("settings.json"), bytes)?;
        assert!(
            matches!(
                Settings::load(&dir),
                Err(Error::Store {
                    action: "Nie można odczytać ustawień",
                    ..
                })
            ),
            "bad settings must surface an operator-visible storage error"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn rejected_settings_leave_the_previous_pairing_untouched() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    settings().save(&dir)?;
    let before = fs::read(dir.file("settings.json"))?;
    for token in [
        "has space".to_owned(),
        "zażółć".to_owned(),
        "x".repeat(4097),
    ] {
        let invalid = Settings {
            token: token.into(),
            ..settings()
        };
        let expected = "Sparuj drukarkę kodem z Holuto w Ustawieniach.";
        assert!(
            matches!(invalid.save(&dir), Err(Error::Invalid(message)) if message == expected),
            "invalid tokens are rejected without disclosure"
        );
    }
    let invalid = Settings {
        baud_rate: 0,
        ..settings()
    };
    let expected = "Nieprawidłowa prędkość transmisji.";
    assert!(
        matches!(invalid.save(&dir), Err(Error::Invalid(message)) if message == expected),
        "unsupported device speed is refused"
    );
    for api_url in [
        "ftp://server",
        "http://server",
        "https://user:pass@server",
        "https://:secret@server",
        "https://server?token=secret",
        "https://server/#fragment",
        "http://localhost.evil.invalid",
        "http://localhost.",
        "http://126.255.255.255",
        "http://128.0.0.0",
        "http://[::]",
        "http://[::2]",
        "http://[::ffff:127.0.0.1]",
        "not a url",
    ] {
        let invalid = Settings {
            api_url: api_url.to_owned(),
            ..settings()
        };
        let expected = "Adres API wymaga https (http tylko dla localhost), bez danych logowania, \
                        zapytania i fragmentu.";
        assert!(
            matches!(invalid.save(&dir), Err(Error::Invalid(message)) if message == expected),
            "unsafe API endpoint must be refused: {api_url}"
        );
    }
    assert_eq!(
        fs::read(dir.file("settings.json"))?,
        before,
        "failed saves never replace valid settings"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn secure_and_loopback_api_addresses_remain_usable() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    for api_url in [
        "https://api.example/v1/",
        "http://localhost:8000",
        "http://127.0.0.1:8000/",
        "http://127.0.0.0",
        "http://127.255.255.255",
        "http://LOCALHOST",
        "http://[::1]:8000",
    ] {
        let expected = Settings {
            api_url: api_url.to_owned(),
            ..settings()
        };
        expected.save(&dir)?;
        assert_eq!(
            Settings::load(&dir)?,
            Some(expected),
            "accepted endpoint must remain usable: {api_url}"
        );
    }
    Ok(())
}

#[cfg(not(windows))]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn startup_reads_opt_out_without_locking_or_creating_state() -> TestResult {
    const MARKER: &str = "HOLUTO_TEST_TELEMETRY_STARTUP";
    if env::var_os(MARKER).is_some() {
        let path = StateDir::default_path().ok_or("no default state directory")?;
        assert!(!path.exists(), "startup must begin with no state");
        assert!(
            Settings::telemetry_enabled_at_default_path(),
            "absent settings opt in"
        );
        assert!(
            !path.exists(),
            "startup must not create the directory or lock"
        );
        let dir = StateDir::open(path)?;
        settings().save(&dir)?;
        assert!(
            !Settings::telemetry_enabled_at_default_path(),
            "crash-reporter startup must honor opt-out while the app holds its lock"
        );
        fs::write(dir.file("settings.json"), "{")?;
        assert!(
            !Settings::telemetry_enabled_at_default_path(),
            "unreadable settings must fail closed for telemetry"
        );
        return Ok(());
    }
    let temporary = tempfile::tempdir()?;
    #[expect(
        clippy::disallowed_methods,
        reason = "Isolated tests re-execute only this test binary; the app never shells out."
    )]
    let mut command = Command::new(env::current_exe()?);
    let _command = command
        .args([
            "settings::tests::startup_reads_opt_out_without_locking_or_creating_state",
            "--exact",
            "--nocapture",
        ])
        .env(MARKER, "1")
        .env("HOME", temporary.path())
        .env("XDG_STATE_HOME", temporary.path().join("stan"));
    let child = command.output()?;
    assert!(child.status.success(), "startup child failed: {child:?}");
    #[cfg(target_os = "macos")]
    let state = temporary
        .path()
        .join("Library/Application Support/workshop");
    #[cfg(not(target_os = "macos"))]
    let state = temporary.path().join("stan/workshop");
    assert_eq!(fs::read(state.join("settings.json"))?, b"{");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn corrupt_settings_never_echo_credentials_from_values_or_unknown_keys() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let secret = "secret-token";
    for bytes in [
        r#"{"token":"secret-token","telemetryEnabled":"secret-token"}"#,
        r#"{"token":"secret-token","baudRate":"secret-token"}"#,
        r#"{"token":"secret-token","secret-token":true}"#,
    ] {
        fs::write(dir.file("settings.json"), bytes)?;
        let error = Settings::load(&dir)
            .err()
            .ok_or("corrupt settings unexpectedly loaded")?;
        assert!(
            !error.to_string().contains(secret),
            "operator errors must never echo the credential"
        );
        assert!(
            !format!("{error:?}").contains(secret),
            "debug errors must never echo the credential"
        );
        match error {
            Error::Store { action, source, .. } => {
                assert_eq!(
                    action, "Nie można odczytać ustawień",
                    "the failure must remain an explicit storage error"
                );
                assert_eq!(
                    source.kind(),
                    ErrorKind::InvalidData,
                    "malformed input remains distinguishable from I/O failure"
                );
                let message = source.to_string();
                assert!(
                    message.starts_with("Nieprawidłowy JSON ustawień (Data, wiersz 1, kolumna "),
                    "safe diagnostics retain the parser category and location"
                );
                assert!(
                    !format!("{source:?}").contains(secret),
                    "the retained source must also be sanitized"
                );
            },
            other @ (Error::Printer(_) | Error::Api(_) | Error::Invalid(_) | Error::Blocked(_)) => {
                return Err(format!("unexpected settings error: {other:?}").into());
            },
        }
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn an_unconfigured_installation_can_persist_a_telemetry_opt_out() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("Ustawienia – Łódź");
    let dir = StateDir::open(path.clone())?;
    Settings::save_telemetry_enabled(&dir, false)?;
    let expected = Settings {
        telemetry_enabled: false,
        ..Settings::default()
    };
    assert_eq!(
        Settings::load(&dir)?,
        Some(expected.clone()),
        "opting out must not require a token, URL, or printer port"
    );
    expected.save(&dir)?;
    assert!(
        matches!(expected.validate(), Err(Error::Invalid(_))),
        "saving preferences does not make an unpaired installation ready"
    );
    drop(dir);
    let reopened = StateDir::open(path)?;
    assert_eq!(
        Settings::load(&reopened)?,
        Some(expected),
        "the unconfigured opt-out must survive restart"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn toggling_telemetry_preserves_every_saved_connection_field() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    settings().save(&dir)?;
    Settings::save_telemetry_enabled(&dir, true)?;
    let expected = Settings {
        telemetry_enabled: true,
        ..settings()
    };
    assert_eq!(
        Settings::load(&dir)?,
        Some(expected),
        "changing telemetry must not replace saved pairing or printer configuration"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions panic on violated guarantees; setup errors use Result."
)]
fn a_telemetry_toggle_does_not_replace_corrupt_settings_with_defaults() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    fs::write(dir.file("settings.json"), "{")?;
    assert!(
        matches!(
            Settings::save_telemetry_enabled(&dir, false),
            Err(Error::Store { .. })
        ),
        "corrupt connection settings must remain an explicit error"
    );
    assert_eq!(
        fs::read(dir.file("settings.json"))?,
        b"{",
        "toggling telemetry must not destroy existing unreadable pairing evidence"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn partial_configuration_saves_without_pairing_and_preserves_autostart() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let settings = Settings {
        start_on_launch: true,
        ..Settings::default()
    };
    settings.validate_configuration()?;
    settings.save(&dir)?;
    assert_eq!(Settings::load_at(dir.path())?, Some(settings.clone()));
    assert!(matches!(settings.validate(), Err(Error::Invalid(_))));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn invalid_port_names_never_replace_saved_preferences() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    settings().save(&dir)?;
    let before = fs::read(dir.file("settings.json"))?;
    for port in ["COM1\0", "/dev/tty\nUSB0", "port\u{0085}"] {
        let invalid = Settings {
            port: port.to_owned(),
            ..settings()
        };
        let expected = "Nieprawidłowa nazwa portu drukarki.";
        assert!(matches!(invalid.save(&dir), Err(Error::Invalid(message)) if message == expected));
    }
    assert_eq!(fs::read(dir.file("settings.json"))?, before);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove vendor validation at the persisted boundary."
)]
fn vendor_preferences_roundtrip_and_invalid_vendors_do_not_load() -> TestResult {
    use fiscal::device::Vendor;

    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    for vendor in [
        None,
        Some(Vendor::Posnet),
        Some(Vendor::Novitus),
        Some(Vendor::Elzab),
    ] {
        let expected = Settings {
            vendor,
            ..settings()
        };
        expected.save(&dir)?;
        assert_eq!(Settings::load(&dir)?, Some(expected));
    }
    let current = include_str!("fixtures/settings.json");
    let invalid_vendors = ["\"other\"", "\"POSNET\"", "1", "true", "{}"]
        .map(|invalid| current.replace("\"vendor\": null", &format!("\"vendor\": {invalid}")));
    for text in invalid_vendors
        .into_iter()
        .chain([current.replace("  \"vendor\": null,\n", "")])
    {
        fs::write(dir.file("settings.json"), &text)?;
        assert!(matches!(Settings::load(&dir), Err(Error::Store { .. })));
        assert_eq!(fs::read(dir.file("settings.json"))?, text.as_bytes());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Stored credential assertions use Result for setup."
)]
fn changing_the_api_forgets_pairing_but_equivalent_addresses_preserve_it() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    for (address, keeps_token) in [
        (" https://API.EXAMPLE:443/v1/// ", true),
        ("https://api.example/v1", true),
        ("https://api.example/v2", false),
        ("https://other.example/v1/", false),
        ("http://localhost/v1/", false),
    ] {
        let mut changed = settings();
        changed.set_api_url(address);
        changed.save(&dir)?;
        let restored = Settings::load(&dir)?.ok_or("missing settings")?;
        assert_eq!(restored.api_url, address);
        assert_eq!(
            restored.token.as_str(),
            if keeps_token { "secret-token" } else { "" }
        );
    }
    for (address, keeps_token) in [("not a url", true), ("another invalid URL", false)] {
        let mut changed = Settings {
            api_url: "not a url".to_owned(),
            ..settings()
        };
        changed.set_api_url(address);
        assert_eq!(
            changed.token.as_str(),
            if keeps_token { "secret-token" } else { "" }
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Missing-state and validation assertions use Result."
)]
fn settings_reads_create_nothing_and_unpaired_preferences_cannot_start() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("absent");
    assert_eq!(Settings::load_at(&path)?, None);
    assert!(!path.exists());
    let dir = StateDir::open(path)?;
    for (url, expected) in [
        ("", "Wpisz adres API w Ustawieniach."),
        (
            "https://api.example",
            "Sparuj drukarkę kodem z Holuto w Ustawieniach.",
        ),
    ] {
        let preferences = Settings {
            api_url: url.to_owned(),
            ..Settings::default()
        };
        preferences.save(&dir)?;
        let loaded = Settings::load(&dir)?.ok_or("missing preferences")?;
        assert!(matches!(loaded.validate(), Err(Error::Invalid(message)) if message == expected));
        assert!(loaded.telemetry_enabled);
        assert!(!loaded.start_on_launch);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unix privacy assertions use Result for setup."
)]
fn settings_creation_and_replacement_are_private_but_reads_do_not_repair_modes() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let path = dir.file("settings.json");
    settings().save(&dir)?;
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
    assert_eq!(Settings::load(&dir)?, Some(settings()));
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o644);
    settings().save(&dir)?;
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The documented token-size boundary must remain usable."
)]
fn the_largest_permitted_credential_saves_without_truncation() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let dir = StateDir::open(temporary.path().join("state"))?;
    let preferences = Settings {
        token: "x".repeat(4096).into(),
        ..settings()
    };
    preferences.save(&dir)?;
    assert_eq!(Settings::load(&dir)?, Some(preferences));
    Ok(())
}
