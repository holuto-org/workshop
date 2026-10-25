//! Display cache, not the fiscal record. Cache errors never block receipt settlement.
//!
//! Appends atomically compact JSONL to 500 distinct receipts; reads skip torn lines
//! with bounded state. Replay may fill a missing receipt number, never replace print evidence.

use std::{
    collections::VecDeque,
    fmt,
    fs::File,
    io::{self, BufRead as _, BufReader},
    path::Path,
};

use fiscal::{money::Grosze, receipt::PaymentForm};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Error,
    store::{self, StateDir},
};

const FILE: &str = "history.jsonl";
const LIMIT: usize = 500;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl fmt::Debug for HistoryEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HistoryEntry")
            .field("job_id", &self.job_id)
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

/// Newest first. GUI reads only before worker startup, then uses snapshots.
pub fn load(dir: &StateDir) -> Result<Vec<HistoryEntry>, Error> {
    let path = dir.file(FILE);
    let entries = read_entries(&path).map_err(|source| Error::Store {
        action: "Nie można odczytać historii",
        path,
        source,
    })?;
    Ok(entries.into_iter().rev().collect())
}

/// Returns a newest-first snapshot after the compact file is durable.
pub(crate) fn append(dir: &StateDir, entry: &HistoryEntry) -> Result<Vec<HistoryEntry>, Error> {
    let path = dir.file(FILE);
    let failed = |source| Error::Store {
        action: "Nie można zapisać historii",
        path: path.clone(),
        source,
    };
    let mut entries = read_entries(&path).map_err(failed)?;
    retain(&mut entries, entry.clone());
    let mut bytes = Vec::new();
    for saved in &entries {
        serde_json::to_writer(&mut bytes, saved)
            .map_err(|error| failed(io::Error::other(error)))?;
        bytes.push(b'\n');
    }
    store::write_durably(&path, &bytes).map_err(failed)?;
    Ok(entries.into_iter().rev().collect())
}

fn read_entries(path: &Path) -> io::Result<VecDeque<HistoryEntry>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(VecDeque::new()),
        Err(error) => return Err(error),
    };
    store::make_private(&file)?;
    let mut entries = VecDeque::new();
    for line in BufReader::new(file).split(b'\n') {
        let line = line?;
        if let Ok(entry) = serde_json::from_slice(&line) {
            retain(&mut entries, entry);
        }
    }
    Ok(entries)
}

fn retain(entries: &mut VecDeque<HistoryEntry>, entry: HistoryEntry) {
    if let Some(known) = entries
        .iter_mut()
        .find(|known| known.job_id == entry.job_id)
    {
        if known.receipt_number.is_none() {
            known.receipt_number = entry.receipt_number;
        }
    } else {
        if entries.len() == LIMIT {
            let _oldest = entries.pop_front();
        }
        entries.push_back(entry);
    }
}

#[cfg(test)]
mod tests;
