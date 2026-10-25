//! Display snapshots, not fiscal authority; the desktop's persistence writer publishes them.
//!
//! Readers check the instance lock before and after loading and ignore snapshots while the
//! app is stopped. Loading takes no lock and creates no state. Disconnected snapshots may
//! retain printer identity and unresolved flags. Never stores credentials, API URLs,
//! customer data or document payloads.

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
    pub vendor: Option<Vendor>,
    pub unique_number: Option<String>,
    /// Holuto identity, not the physical device's unique number.
    pub printer_id: Option<String>,
    pub blocked: bool,
    pub report_unresolved: bool,
    pub orphaned_receipt: bool,
    /// Set when state changes, not when saved or loaded.
    pub updated_at: Timestamp,
}

impl Snapshot {
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
                // Serde errors may expose malformed values or keys.
                unreadable(io::Error::new(
                    ErrorKind::InvalidData,
                    "Nieprawidłowy JSON stanu aplikacji.",
                ))
            })
    }

    /// Durable atomic publication; private (0600) on Unix.
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
