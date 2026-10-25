//! Contains the device token; stored privately (0600 on Unix).

use std::{
    fmt,
    io::{self, ErrorKind},
    net::IpAddr,
    path::Path,
};

use fiscal::device::Vendor;
use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::{
    Error,
    store::{self, StateDir},
};

pub const BAUD_RATES: [u32; 5] = [9600, 19200, 38400, 57600, 115_200];
pub const DEFAULT_BAUD_RATE: u32 = 9600;
const FILE: &str = "settings.json";
const MAX_TOKEN_LENGTH: usize = 4096;

/// Device credential. Formatting never exposes it; storage remains a JSON string.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Token(String);

impl Token {
    pub const fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for Token {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Token {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub api_url: String,
    pub token: Token,
    /// Empty until a printer port is chosen or detected.
    pub port: String,
    pub baud_rate: u32,
    /// None selects read-only vendor detection; legacy settings have no preference.
    #[serde(default)]
    pub vendor: Option<Vendor>,
    /// Optional diagnostic reporting; enabled until explicitly disabled.
    pub telemetry_enabled: bool,
    /// Opt-in processing at launch.
    pub start_on_launch: bool,
}

/// Unconfigured, at the printer's factory baud rate.
impl Default for Settings {
    fn default() -> Self {
        Self {
            api_url: String::new(),
            token: Token::default(),
            port: String::new(),
            baud_rate: DEFAULT_BAUD_RATE,
            vendor: None,
            telemetry_enabled: true,
            start_on_launch: false,
        }
    }
}

impl Settings {
    /// Reads only settings: safe before telemetry initialization, including a crash
    /// reporter subprocess while the application holds the instance lock.
    /// Missing settings opt in; unreadable or malformed settings fail closed.
    pub fn telemetry_enabled_at_default_path() -> bool {
        let Some(path) = StateDir::default_path() else {
            return false;
        };
        match Self::load_at(&path) {
            Ok(Some(settings)) => settings.telemetry_enabled,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    pub fn load(dir: &StateDir) -> Result<Option<Self>, Error> {
        Self::load_at(dir.path())
    }

    /// Reads settings without acquiring an instance lock or creating state.
    pub fn load_at(dir: &Path) -> Result<Option<Self>, Error> {
        let path = dir.join(FILE);
        let unreadable = |source| Error::Store {
            action: "Nie można odczytać ustawień",
            path: path.clone(),
            source,
        };
        let Some(bytes) = store::read_optional(&path).map_err(unreadable)? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes).map(Some).map_err(|error| {
            // Serde's message can echo a credential from any malformed value or key.
            unreadable(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "Nieprawidłowy JSON ustawień ({:?}, wiersz {}, kolumna {}).",
                    error.classify(),
                    error.line(),
                    error.column()
                ),
            ))
        })
    }

    pub fn save(&self, dir: &StateDir) -> Result<(), Error> {
        self.validate_configuration()?;
        self.write(dir)
    }

    /// Persists diagnostics consent independently of printer pairing validation.
    /// A new installation must be able to opt out before entering credentials.
    pub fn save_telemetry_enabled(dir: &StateDir, enabled: bool) -> Result<(), Error> {
        let mut settings = Self::load(dir)?.unwrap_or_default();
        settings.telemetry_enabled = enabled;
        settings.write(dir)
    }

    fn write(&self, dir: &StateDir) -> Result<(), Error> {
        let path = dir.file(FILE);
        let bytes =
            serde_json::to_vec_pretty(self).map_err(|error| Error::Invalid(error.to_string()))?;
        store::write_durably(&path, &bytes).map_err(|source| Error::Store {
            action: "Nie można zapisać ustawień",
            path,
            source,
        })
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.api_url.trim().is_empty() {
            return Err(Error::Invalid("Wpisz adres API w Ustawieniach.".to_owned()));
        }
        self.validate_configuration()?;
        if self.token.as_str().is_empty() {
            return Err(Error::Invalid(
                "Sparuj drukarkę kodem z Holuto w Ustawieniach.".to_owned(),
            ));
        }
        Ok(())
    }

    /// Validates saved preferences; URL and pairing may still be unconfigured.
    pub fn validate_configuration(&self) -> Result<(), Error> {
        if !self.api_url.is_empty() {
            let _normalized = api_url(&self.api_url)?;
        }
        let token = self.token.as_str();
        if token.len() > MAX_TOKEN_LENGTH || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(Error::Invalid(
                "Sparuj drukarkę kodem z Holuto w Ustawieniach.".to_owned(),
            ));
        }
        if self.port.chars().any(char::is_control) {
            return Err(Error::Invalid(
                "Nieprawidłowa nazwa portu drukarki.".to_owned(),
            ));
        }
        if !BAUD_RATES.contains(&self.baud_rate) {
            return Err(Error::Invalid(
                "Nieprawidłowa prędkość transmisji.".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Preserves path prefixes and strips trailing slashes. HTTP is allowed only on loopback;
/// credentials, queries and fragments are rejected.
pub(crate) fn api_url(text: &str) -> Result<String, Error> {
    let refused = || {
        Error::Invalid(
            "Adres API wymaga https (http tylko dla localhost), bez danych logowania, zapytania i \
             fragmentu."
                .to_owned(),
        )
    };
    let url = Url::parse(text.trim()).map_err(|_unparsable| refused())?;
    let loopback = match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
        Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
        None => return Err(refused()),
    };
    let scheme_allowed = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    if !scheme_allowed
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(refused());
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests;
