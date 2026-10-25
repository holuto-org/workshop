//! Read-only application display status, never journal recovery or fiscal authority.
//!
//! The desktop publishes its actual state through its persistence writer; readers
//! must check the instance lock before and after loading, and ignore stale snapshots
//! when the application is not running. Loading acquires no lock and creates nothing.
//! Disconnection may retain the last known printer identity and unresolved flags;
//! those values describe the last published state, not permission to operate a printer.
//! Contains no credentials, API URLs, customer data or document payloads.

use std::{
    io::{self, ErrorKind},
    path::Path,
};

use fiscal::device::Vendor;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::{
    Error,
    store::{self, StateDir},
};

const FILE: &str = "status.json";

#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent worker capability and block flags"
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub processing: bool,
    pub connected: bool,
    /// Last known physical device vendor; may remain set while disconnected.
    pub vendor: Option<Vendor>,
    /// Last known physical device serial; may remain set while disconnected.
    pub serial: Option<String>,
    /// Last known Holuto printer identity; may remain set while disconnected.
    pub printer_id: Option<String>,
    pub blocked: bool,
    pub report_unresolved: bool,
    pub orphaned_receipt: bool,
    /// Set by the publisher when the state changes, not by readers or persistence.
    pub updated_at: Timestamp,
}

impl Snapshot {
    /// Reads display status without taking the instance lock or opening the journal.
    pub fn load_at(directory: &Path) -> Result<Option<Self>, Error> {
        let path = directory.join(FILE);
        let unreadable = |source| Error::Store {
            action: "Nie można odczytać stanu aplikacji",
            path: path.clone(),
            source,
        };
        let Some(bytes) = store::read_optional(&path).map_err(unreadable)? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_malformed| {
                // Serde errors may echo arbitrary malformed values or unknown keys.
                unreadable(io::Error::new(
                    ErrorKind::InvalidData,
                    "Nieprawidłowy JSON stanu aplikacji.",
                ))
            })
    }

    /// Atomically publishes a durable snapshot, private (0600) on Unix.
    pub fn save(&self, directory: &StateDir) -> Result<(), Error> {
        let path = directory.file(FILE);
        let bytes = serde_json::to_vec_pretty(self).map_err(|_encoding| {
            Error::Invalid("Nie można zakodować stanu aplikacji.".to_owned())
        })?;
        store::write_durably(&path, &bytes).map_err(|source| Error::Store {
            action: "Nie można zapisać stanu aplikacji",
            path,
            source,
        })
    }
}

#[cfg(test)]
mod tests;
