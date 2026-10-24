//! Posnet Temo Online driver over a serial port.
//!
//! Each operation opens an exclusive session and verifies identity, fiscal mode, header,
//! mechanism, command queue, transaction state and VAT table before fiscal commands.
//! Commands are never retried. A lost reply leaves an uncertain outcome for the caller to resolve.
//! The caller's [`printer::Journal`] persists intent before each command, surviving restarts.

#![forbid(unsafe_code)]

pub mod decimal;
pub mod frame;
pub mod money;
pub mod printer;
pub mod receipt;
pub mod session;
#[cfg(any(test, feature = "simulator"))]
pub mod simulator;
pub mod text;

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Refused before any command reached the printer.
    #[error("{0}")]
    Invalid(String),
    #[error("Nie wykryto drukarki.")]
    NoPort,
    #[error("Nie można połączyć z drukarką {port}: {source}")]
    Open {
        port: String,
        #[source]
        source: io::Error,
    },
    #[error("Przerwano połączenie z drukarką: {0}")]
    Io(#[source] io::Error),
    #[error("Brak odpowiedzi na {0}. Polecenie nie zostało ponowione.")]
    Timeout(&'static str),
    #[error(
        "Drukarka zwróciła błąd {code} ({command}{}).{}",
        parameter(.field),
        explanation(*.code)
    )]
    Device {
        code: u32,
        command: String,
        field: String,
        /// `ERR` rejects a frame before execution; command-named errors may follow effects.
        rejected: bool,
    },
    /// Corrupt, incomplete or mismatched reply.
    #[error("{0}")]
    Protocol(String),
    /// Operator intervention required (paper, cover, open transaction, …).
    #[error("{0}")]
    NotReady(String),
    /// Journal write failed; the next command was not sent.
    #[error("Nie można zapisać rejestru operacji: {0}")]
    Journal(#[source] io::Error),
    /// Commands reached the printer; the printed outcome is unknown.
    #[error(
        "{0}\nWynik operacji wymaga sprawdzenia. Nie wysyłaj jej ponownie przed sprawdzeniem \
         drukarki."
    )]
    Uncertain(#[source] Box<Self>),
}

impl Error {
    /// Port permission denied; Linux usually requires membership in `dialout`.
    pub fn is_denied(&self) -> bool {
        let Self::Open { source, .. } = self else {
            return false;
        };
        source.kind() == io::ErrorKind::PermissionDenied
    }
}

fn parameter(field: &str) -> String {
    if field.is_empty() {
        String::new()
    } else {
        format!(", parametr {field}")
    }
}

/// Errors preflight cannot detect (POT-I-DEV-37 v5978, "Opisy błędów").
const fn explanation(code: u32) -> &'static str {
    match code {
        1039 | 2904 => " Pamięć chroniona zapełniona.",
        2036 => " Urządzenie w stanie tylko do odczytu.",
        2098 => " Pamięć fiskalna zapełniona.",
        3728 => " Upłynął termin ważności certyfikatu.",
        3733 => " Sprzedaż zablokowana. Brak przekazanych kluczy do Repozytorium.",
        3735 => " Sprzedaż zablokowana. Wgraj nowe certyfikaty.",
        3903 => " Operacja w toku. Spróbuj ponownie.",
        3929 => " Sprzedaż zablokowana. Brak klucza współdzielonego.",
        _ => "",
    }
}

#[cfg(test)]
mod tests;
