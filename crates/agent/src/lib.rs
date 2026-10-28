//! Fiscal operations run on one worker thread.
//!
//! Journal intent before dispatch; never retry device commands or reprint.
//! Uncertain outcomes block receipts and reports across restarts until staff resolve
//! receipts in Holuto or an operator confirms reports on the same device.
//! Resolution never reissues commands.

#![forbid(unsafe_code)]

#[cfg(all(feature = "testing", not(debug_assertions)))]
compile_error!("The testing feature must never be enabled in release builds.");

pub mod api;
pub mod device;
pub mod settings;
pub mod status;
pub mod store;
pub mod worker;

mod agent;
mod journal;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

use std::{fmt, io, path::PathBuf};

use fiscal::error::Error as PrinterError;
use i18n::Message;

use api::ApiError;

#[derive(thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Printer(#[from] PrinterError),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("{action} ({path}): {source}")]
    Store {
        action: Message,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Operator-correctable input.
    #[error("{0}")]
    Invalid(Message),
    /// Requires human resolution before fiscal operations can continue.
    #[error("{0}")]
    Blocked(Message),
    /// Another `workshop` command holds the state directory; retry once it finishes.
    #[error("{0}")]
    Busy(Message),
}

impl Error {
    pub fn message(&self) -> Message {
        match self {
            Self::Printer(error) => error.message(),
            Self::Api(error) => error.message(),
            Self::Store {
                action,
                path,
                source,
            } => Message::new("agent-store-error")
                .arg("action", action.clone())
                .arg("path", path.display().to_string())
                .arg(
                    "detail",
                    source
                        .get_ref()
                        .and_then(|error| error.downcast_ref::<Message>())
                        .cloned()
                        .unwrap_or_else(|| Message::literal(source.to_string())),
                ),
            Self::Invalid(message) | Self::Blocked(message) | Self::Busy(message) => {
                message.clone()
            },
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Printer(_) => f.write_str("Printer([REDACTED])"),
            Self::Api(error) => f.debug_tuple("Api").field(error).finish(),
            Self::Store { source, .. } => f
                .debug_struct("Store")
                .field("kind", &source.kind())
                .finish_non_exhaustive(),
            Self::Invalid(_) => f.write_str("Invalid([REDACTED])"),
            Self::Blocked(_) => f.write_str("Blocked([REDACTED])"),
            Self::Busy(_) => f.write_str("Busy([REDACTED])"),
        }
    }
}
