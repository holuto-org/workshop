//! Display cache, not the fiscal record. Cache errors never block receipt settlement.

use std::{
    fs::File,
    io::{self, Write as _},
};

use jiff::Timestamp;
use posnet::{money::Grosze, receipt::PaymentForm};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Error,
    store::{self, StateDir},
};

const FILE: &str = "history.jsonl";
const LIMIT: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub job_id: Uuid,
    pub printed_at: Timestamp,
    /// Printer's number, or the number staff supplied in Holuto.
    pub receipt_number: Option<String>,
    pub total: Grosze,
    pub payments: Vec<(PaymentForm, Grosze)>,
    pub items: Vec<String>,
}

/// Up to 500 receipts, newest first; skips crash-torn lines.
pub fn load(dir: &StateDir) -> Result<Vec<HistoryEntry>, Error> {
    let path = dir.file(FILE);
    let mut entries = store::read_optional(&path)
        .map_err(|source| Error::Store {
            action: "Nie można odczytać historii",
            path,
            source,
        })?
        .map(|bytes| parse(&bytes))
        .unwrap_or_default();
    entries.reverse();
    entries.truncate(LIMIT);
    Ok(entries)
}

pub(crate) fn append(dir: &StateDir, entry: &HistoryEntry) -> Result<(), Error> {
    let path = dir.file(FILE);
    let failed = |source| Error::Store {
        action: "Nie można zapisać historii",
        path: path.clone(),
        source,
    };
    let existing = store::read_optional(&path)
        .map_err(failed)?
        .unwrap_or_default();
    if parse(&existing)
        .iter()
        .any(|known| known.job_id == entry.job_id)
    {
        return Ok(());
    }
    let mut line = Vec::new();
    if existing.last().is_some_and(|byte| *byte != b'\n') {
        line.push(b'\n'); // Keep a torn line from swallowing this entry.
    }
    serde_json::to_writer(&mut line, entry).map_err(|error| failed(io::Error::other(error)))?;
    line.push(b'\n');
    let mut file = File::options()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(failed)?;
    file.write_all(&line)
        .and_then(|()| file.sync_all())
        .map_err(failed)
}

fn parse(bytes: &[u8]) -> Vec<HistoryEntry> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}

#[cfg(test)]
mod tests;
