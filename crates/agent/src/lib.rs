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
pub mod documents;
pub mod history;
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

use api::ApiError;

#[derive(thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Printer(#[from] PrinterError),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("{action} ({path}): {source}")]
    Store {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Operator-correctable input.
    #[error("{0}")]
    Invalid(String),
    /// Requires human resolution before fiscal operations can continue.
    #[error("{0}")]
    Blocked(String),
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
        }
    }
}
