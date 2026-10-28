use std::{fmt, io};

use i18n::Message;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Refused before any command reached the printer.
    Invalid(Message),
    /// Unsupported identification; nothing else was sent.
    Unsupported(Message),
    NoPort,
    Open {
        port: String,
        source: io::Error,
    },
    Io(#[source] io::Error),
    /// The command is a raw protocol token, never translated.
    Timeout(&'static str),
    Device {
        code: u32,
        command: String,
        /// Complete, language-neutral explanation supplied by the driver.
        detail: Message,
        /// `ERR` rejects a frame before execution; command-named errors may follow effects.
        rejected: bool,
    },
    /// Corrupt, incomplete or mismatched reply.
    Protocol(Message),
    /// Operator intervention required (paper, cover, open transaction, …).
    NotReady(Message),
    /// Journal write failed; the next command was not sent.
    Journal(#[source] io::Error),
    /// Commands reached the printer; the printed outcome is unknown.
    Uncertain(#[source] Box<Self>),
}

impl Error {
    /// Render only at the consumer boundary; nested fiscal warnings remain translatable.
    pub fn message(&self) -> Message {
        match self {
            Self::Invalid(message)
            | Self::Unsupported(message)
            | Self::Protocol(message)
            | Self::NotReady(message)
            | Self::Device {
                detail: message, ..
            } => message.clone(),
            Self::NoPort => Message::new("fiscal-no-port"),
            Self::Open { port, source } => Message::new("fiscal-open")
                .arg("port", port.as_str())
                .arg("source", source.to_string()),
            Self::Io(source) => Message::new("fiscal-io").arg("detail", source.to_string()),
            Self::Timeout(command) => Message::new("fiscal-timeout").arg("command", *command),
            Self::Journal(source) => {
                Message::new("fiscal-journal").arg("detail", source.to_string())
            },
            Self::Uncertain(source) => {
                Message::new("fiscal-uncertain").arg("detail", source.message())
            },
        }
    }

    /// Port permission denied; Linux usually requires membership in `dialout`.
    pub fn is_denied(&self) -> bool {
        let Self::Open { source, .. } = self else {
            return false;
        };
        source.kind() == io::ErrorKind::PermissionDenied
    }
}

/// Polish is the stable CLI/persisted diagnostic contract, independent of UI language.
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.message(), f)
    }
}
