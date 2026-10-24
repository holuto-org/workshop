//! Fiscal operations run on one worker thread.
//!
//! Journal intent before dispatch; never retry device commands or reprint.
//! Uncertain outcomes block receipts and reports across restarts until staff resolve
//! receipts in Holuto or an operator confirms reports on the same device.
//! Resolution never reissues commands.

#![forbid(unsafe_code)]

pub mod api;
pub mod documents;
pub mod history;
pub mod settings;
pub mod store;
pub mod worker;

mod agent;
mod journal;
#[cfg(test)]
mod testing;

use std::{io, path::PathBuf};

use api::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Printer(#[from] posnet::Error),
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
