//! The durable record of the last fiscal operation.
//!
//! It is written before the first command reaches the printer, updated before every command, and
//! kept until the outcome is settled. A journal that exists but cannot be read is never
//! "nothing": printing stays blocked.

use std::{io, path::PathBuf};

use jiff::Timestamp;
use posnet::{
    money::Grosze,
    printer::Report,
    receipt::{Payment, Receipt},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Error,
    api::model::{JobState, Outcome, ResultForm},
    store::{self, StateDir},
};

const FILE: &str = "operation.json";
/// The backend's `Note` limit for a result text.
const MAX_REASON_LENGTH: usize = 2000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Operation {
    Receipt(ReceiptOperation),
    Report(ReportOperation),
}

/// How far the printer got.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Device {
    /// Nothing was sent yet: the claim is recorded but preflight is still running.
    NotStarted,
    /// `stage` is about to be sent or was sent; what the printer did is unknown.
    Dispatching { stage: String },
    /// Every command succeeded; the counter may not have been readable.
    Completed { receipt_number: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiptOperation {
    pub(crate) job_id: Uuid,
    pub(crate) attempt: Uuid,
    /// The execution start; a `done` result reports it as the issue time, resent unchanged.
    pub(crate) issued_at: Timestamp,
    pub(crate) device_serial: String,
    pub(crate) device: Device,
    /// What was sent to the printer, for the local history.
    pub(crate) summary: Option<Summary>,
    /// The result, persisted before it is sent and resent unchanged until accepted.
    pub(crate) result: Option<JobResult>,
    pub(crate) settled: Option<Settlement>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Summary {
    pub(crate) total: Grosze,
    pub(crate) payments: Vec<Payment>,
    pub(crate) items: Vec<String>,
}

impl Summary {
    pub(crate) fn of(receipt: &Receipt) -> Self {
        Self {
            total: receipt.fiscal_total(),
            payments: receipt.payments().to_vec(),
            items: receipt
                .lines()
                .iter()
                .map(|line| line.name().to_owned())
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum JobResult {
    Done { receipt_number: String },
    Failed { reason: String },
    Unknown { reason: String },
}

impl JobResult {
    pub(crate) fn failed(reason: &str) -> Self {
        Self::Failed {
            reason: note(reason),
        }
    }

    pub(crate) fn unknown(reason: &str) -> Self {
        Self::Unknown {
            reason: note(reason),
        }
    }

    pub(crate) const fn outcome(&self) -> Outcome {
        match self {
            Self::Done { .. } => Outcome::Done,
            Self::Failed { .. } => Outcome::Failed,
            Self::Unknown { .. } => Outcome::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settlement {
    pub(crate) at: Timestamp,
    /// The backend state that settled it: the reported result, or a staff resolution.
    pub(crate) state: JobState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReportOperation {
    pub(crate) report: Report,
    pub(crate) device_serial: String,
    pub(crate) started_at: Timestamp,
    pub(crate) device: Device,
    /// An operator confirmed, on the same ready device, what an interrupted report printed.
    pub(crate) acknowledged_at: Option<Timestamp>,
}

impl ReceiptOperation {
    pub(crate) fn result_form(&self) -> Option<ResultForm> {
        let result = self.result.as_ref()?;
        let (receipt_number, issued_at, reason) = match result {
            JobResult::Done { receipt_number } => {
                (Some(receipt_number.clone()), Some(self.issued_at), None)
            },
            JobResult::Failed { reason } | JobResult::Unknown { reason } => {
                (None, None, Some(reason.clone()))
            },
        };
        Some(ResultForm {
            attempt: self.attempt,
            state: result.outcome(),
            receipt_number,
            issued_at,
            result: reason,
        })
    }
}

impl ReportOperation {
    pub(crate) const fn unresolved(&self) -> bool {
        self.acknowledged_at.is_none() && !matches!(self.device, Device::Completed { .. })
    }
}

impl Operation {
    /// Whether this operation still blocks every new receipt and report.
    pub(crate) const fn blocks(&self) -> bool {
        match self {
            Self::Receipt(receipt) => receipt.settled.is_none(),
            Self::Report(report) => report.unresolved(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Journal {
    path: PathBuf,
}

impl Journal {
    pub(crate) fn new(dir: &StateDir) -> Self {
        Self {
            path: dir.file(FILE),
        }
    }

    pub(crate) fn load(&self) -> Result<Option<Operation>, Error> {
        let bytes = store::read_optional(&self.path).map_err(|source| Error::Store {
            action: "Nie można odczytać rejestru operacji. Drukowanie zablokowane",
            path: self.path.clone(),
            source,
        })?;
        bytes
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()
            .map_err(|error| {
                Error::Blocked(format!(
                    "Uszkodzony rejestr operacji ({}): {error}. Drukowanie zablokowane; nie usuwaj \
                     pliku, rozstrzygnij operację w Holuto.",
                    self.path.display()
                ))
            })
    }

    pub(crate) fn save(&self, operation: &Operation) -> Result<(), Error> {
        self.write(operation).map_err(|source| Error::Store {
            action: "Nie można zapisać rejestru operacji",
            path: self.path.clone(),
            source,
        })
    }

    pub(crate) fn write(&self, operation: &Operation) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(operation).map_err(io::Error::other)?;
        store::write_durably(&self.path, &bytes)
    }
}

/// A result text the backend accepts: no control characters but tab and line feed, at most 2000
/// characters, never empty.
fn note(reason: &str) -> String {
    let cleaned: String = reason
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\t' | '\n'))
        .take(MAX_REASON_LENGTH)
        .collect();
    if cleaned.trim().is_empty() {
        "Brak opisu błędu.".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests;
