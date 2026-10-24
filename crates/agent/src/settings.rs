//! Contains the device token; stored privately (0600 on Unix).

use std::net::IpAddr;

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub api_url: String,
    pub token: String,
    /// Empty until a printer port is chosen or detected.
    pub port: String,
    pub baud_rate: u32,
}

/// Unconfigured, at the printer's factory baud rate.
impl Default for Settings {
    fn default() -> Self {
        Self {
            api_url: String::new(),
            token: String::new(),
            port: String::new(),
            baud_rate: DEFAULT_BAUD_RATE,
        }
    }
}

impl Settings {
    pub fn load(dir: &StateDir) -> Result<Option<Self>, Error> {
        let path = dir.file(FILE);
        let unreadable = |source| Error::Store {
            action: "Nie można odczytać ustawień",
            path: path.clone(),
            source,
        };
        let Some(bytes) = store::read_optional(&path).map_err(unreadable)? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| unreadable(error.into()))
    }

    pub fn save(&self, dir: &StateDir) -> Result<(), Error> {
        self.validate()?;
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
        let _normalized = api_url(&self.api_url)?;
        let token_valid = (1..=MAX_TOKEN_LENGTH).contains(&self.token.len())
            && self.token.bytes().all(|byte| byte.is_ascii_graphic());
        if !token_valid {
            return Err(Error::Invalid("Wklej token drukarki z Holuto.".to_owned()));
        }
        if !BAUD_RATES.contains(&self.baud_rate) {
            return Err(Error::Invalid(
                "Nieprawidłowa prędkość transmisji.".to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn remember_port(dir: &StateDir, port: &str) -> Result<(), Error> {
        let Some(mut settings) = Self::load(dir)? else {
            return Ok(());
        };
        if settings.port == port {
            return Ok(());
        }
        port.clone_into(&mut settings.port);
        settings.save(dir)
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
