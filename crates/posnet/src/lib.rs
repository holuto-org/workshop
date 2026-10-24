//! Posnet Temo Online driver over a serial port.
//!
//! Every operation opens its own exclusive session, verifies the device (identity, fiscal mode,
//! header, mechanism, command queue, transaction state, VAT table) and only then sends fiscal
//! commands. No command is ever retried: a lost reply is an uncertain outcome for the caller to
//! resolve, never a reason to send it again. Before each command reaches the printer the caller's
//! [`printer::Journal`] persists it, so an interrupted operation is known after a restart.

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
    #[error("Drukarka zwróciła błąd {code} ({command}{}).", parameter(.field))]
    Device {
        code: u32,
        command: String,
        field: String,
    },
    /// A reply the driver cannot trust: corrupt, incomplete or answering another command.
    #[error("{0}")]
    Protocol(String),
    /// The device refuses work until an operator fixes it (paper, cover, open transaction, ...).
    #[error("{0}")]
    NotReady(String),
    /// The caller's journal could not record the next command, so it was not sent.
    #[error("Nie można zapisać rejestru operacji: {0}")]
    Journal(#[source] io::Error),
    /// Commands of an operation already reached the printer; what it printed is unknown.
    #[error(
        "{0}\nWynik operacji wymaga sprawdzenia. Nie wysyłaj jej ponownie przed sprawdzeniem \
         drukarki."
    )]
    Uncertain(#[source] Box<Self>),
}

impl Error {
    /// The OS refused to open the port; on Linux the user usually lacks the `dialout` group.
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
