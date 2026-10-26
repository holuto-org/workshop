use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Refused before any command reached the printer.
    #[error("{0}")]
    Invalid(String),
    /// A printer answered identification with an unsupported model; nothing else was sent.
    #[error("{0}")]
    Unsupported(String),
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
    #[error("{detail}")]
    Device {
        code: u32,
        command: String,
        /// Complete Polish explanation supplied by the driver.
        detail: String,
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
