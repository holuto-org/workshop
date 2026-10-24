use std::fs;

use super::{DEFAULT_BAUD_RATE, Settings, api_url};
use crate::{Error, testing::state_dir};

fn settings() -> Settings {
    Settings {
        api_url: "https://api.example/v1/".to_owned(),
        token: "secret-token".to_owned(),
        port: "/dev/ttyACM0".to_owned(),
        baud_rate: 38_400,
    }
}

#[test]
fn saved_settings_are_restored_exactly() {
    let (_temporary, dir) = state_dir();
    assert_eq!(Settings::load(&dir).unwrap(), None);
    settings().save(&dir).unwrap();
    assert_eq!(Settings::load(&dir).unwrap(), Some(settings()));
}

#[test]
fn an_unreadable_settings_file_is_an_error_not_empty_settings() {
    let (_temporary, dir) = state_dir();
    fs::write(dir.path().join("settings.json"), "{").unwrap();
    assert!(matches!(Settings::load(&dir), Err(Error::Store { .. })));
}

#[test]
fn invalid_settings_are_never_saved() {
    let (_temporary, dir) = state_dir();
    for invalid in [
        Settings {
            token: String::new(),
            ..settings()
        },
        Settings {
            token: "has space".to_owned(),
            ..settings()
        },
        Settings {
            token: "zażółć".to_owned(),
            ..settings()
        },
        Settings {
            baud_rate: 0,
            ..settings()
        },
        Settings {
            api_url: "http://api.example".to_owned(),
            ..settings()
        },
    ] {
        assert!(
            matches!(invalid.save(&dir), Err(Error::Invalid(_))),
            "{invalid:?}"
        );
    }
    assert_eq!(Settings::load(&dir).unwrap(), None);
}

#[test]
fn the_default_is_unconfigured_at_the_factory_speed() {
    let default = Settings::default();
    assert_eq!(default.baud_rate, DEFAULT_BAUD_RATE);
    assert!(default.validate().is_err());
}

#[test]
fn only_https_or_loopback_http_without_credentials_query_or_fragment() {
    for url in [
        "ftp://server",
        "http://server",
        "https://user:pass@server",
        "https://server?token=secret",
        "https://server/#fragment",
        "not a url",
    ] {
        assert!(api_url(url).is_err(), "{url}");
    }
    for (url, normalized) in [
        ("https://api.example/v1/", "https://api.example/v1"),
        ("http://localhost:8000", "http://localhost:8000"),
        ("http://127.0.0.1:8000/", "http://127.0.0.1:8000"),
        ("http://[::1]:8000", "http://[::1]:8000"),
    ] {
        assert_eq!(api_url(url).unwrap(), normalized);
    }
}
