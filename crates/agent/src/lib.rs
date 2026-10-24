//! Holuto agent: the `/agent` API, the durable operation journal, receipt execution and the single
//! worker thread that owns all printer I/O.
//!
//! Fiscal safety lives here: before any command reaches the printer the claim is journaled; no
//! device command is retried and nothing is reprinted; an uncertain outcome blocks every receipt
//! and report, across restarts, until staff resolve the job in Holuto (receipts) or an operator
//! confirms the printout on the same device (reports). Resolving never reissues anything.

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
    /// Input the operator can correct: settings, a buyer NIP.
    #[error("{0}")]
    Invalid(String),
    /// Fiscal safety refuses to continue until a person resolves the state.
    #[error("{0}")]
    Blocked(String),
}
