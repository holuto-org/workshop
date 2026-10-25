//! Persisted before each printer command and retained until settlement.
//! An unreadable journal blocks printing.

mod legacy;

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{borrow::Cow, fmt, io, marker::PhantomData, path::PathBuf};

use fiscal::{
    device::Vendor,
    money::Grosze,
    receipt::{Payment, Receipt},
    report::Report,
};
use icu_normalizer::ComposingNormalizerBorrowed;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Error,
    api::model::{JobState, Outcome, ResultForm},
    store::{self, StateDir},
};

const FILE: &str = "operation.json";
/// Backend `Note` limit.
const MAX_REASON_LENGTH: usize = 2000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Operation {
    Receipt(ReceiptOperation),
    Report(ReportOperation),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Device {
    /// Claim recorded; no command sent.
    NotStarted,
    /// Command may have reached the printer; outcome unknown.
    Dispatching { stage: String },
    /// All commands succeeded; the counter may be unreadable.
    Completed { receipt_number: Option<String> },
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStarted => f.write_str("NotStarted"),
            Self::Dispatching { stage } => {
                f.debug_struct("Dispatching").field("stage", stage).finish()
            },
            Self::Completed { .. } => f.write_str("Completed([REDACTED])"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiptOperation {
    pub(crate) job_id: Uuid,
    pub(crate) attempt: Uuid,
    /// Claim time until completion, then printer time; resent unchanged.
    /// The server rejects future timestamps.
    pub(crate) issued_at: Timestamp,
    pub(crate) vendor: Vendor,
    pub(crate) device_serial: String,
    pub(crate) device: Device,
    /// Printed contents for local history.
    pub(crate) summary: Option<Summary>,
    /// Persisted before sending; resent unchanged until accepted.
    pub(crate) result: Option<JobResult>,
    pub(crate) settled: Option<Settlement>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Summary {
    pub(crate) total: Grosze,
    pub(crate) payments: Vec<Payment>,
    pub(crate) items: Vec<String>,
}

impl fmt::Debug for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Summary")
            .field("items", &self.items.len())
            .field("payments", &self.payments.len())
            .finish_non_exhaustive()
    }
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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum JobResult {
    Done { receipt_number: String },
    Failed { reason: String },
    Unknown { reason: String },
}

impl fmt::Debug for JobResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Done { .. } => "Done([REDACTED])",
            Self::Failed { .. } => "Failed([REDACTED])",
            Self::Unknown { .. } => "Unknown([REDACTED])",
        })
    }
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
    /// Backend result or staff resolution.
    pub(crate) state: JobState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReportOperation {
    pub(crate) report: Report,
    pub(crate) vendor: Vendor,
    pub(crate) device_serial: String,
    pub(crate) started_at: Timestamp,
    pub(crate) device: Device,
    /// Operator confirmed the interrupted report's printout on the same ready device.
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
            issued_at,
            receipt_number,
            result: reason,
            state: result.outcome(),
        })
    }
}

impl ReportOperation {
    pub(crate) const fn unresolved(&self) -> bool {
        self.acknowledged_at.is_none() && matches!(self.device, Device::Dispatching { .. })
    }
}

impl Operation {
    pub(crate) const fn blocks(&self) -> bool {
        match self {
            Self::Receipt(receipt) => receipt.settled.is_none(),
            Self::Report(report) => report.unresolved(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Journal<'dir> {
    path: PathBuf,
    _lock: PhantomData<&'dir StateDir>,
    #[cfg(test)]
    fail_write: AtomicBool,
}

impl<'dir> Journal<'dir> {
    pub(crate) fn new(dir: &'dir StateDir) -> Self {
        Self {
            path: dir.file(FILE),
            _lock: PhantomData,
            #[cfg(test)]
            fail_write: AtomicBool::new(false),
        }
    }

    pub(crate) fn load(&self) -> Result<Option<Operation>, Error> {
        let bytes = store::read_optional(&self.path).map_err(|source| Error::Store {
            action: "Nie można odczytać rejestru operacji. Drukowanie zablokowane",
            path: self.path.clone(),
            source,
        })?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        match serde_json::from_slice(&bytes) {
            Ok(operation) => Ok(Some(operation)),
            Err(error) => {
                let operation = legacy::decode(&bytes).map_err(|_not_legacy| {
                    Error::Blocked(format!(
                        "Uszkodzony rejestr operacji ({}): {error}. Drukowanie zablokowane; nie \
                         usuwaj pliku, rozstrzygnij operację w Holuto.",
                        self.path.display()
                    ))
                })?;
                self.save(&operation)?;
                Ok(Some(operation))
            },
        }
    }

    pub(crate) fn save(&self, operation: &Operation) -> Result<(), Error> {
        self.write(operation).map_err(|source| Error::Store {
            action: "Nie można zapisać rejestru operacji",
            path: self.path.clone(),
            source,
        })
    }

    #[cfg(test)]
    pub(crate) fn fail_next_write(&self) {
        self.fail_write.store(true, Ordering::SeqCst);
    }

    pub(crate) fn write(&self, operation: &Operation) -> io::Result<()> {
        #[cfg(test)]
        if self.fail_write.swap(false, Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected journal write failure",
            ));
        }
        let bytes = serde_json::to_vec_pretty(operation).map_err(io::Error::other)?;
        store::write_durably(&self.path, &bytes)
    }
}

/// Backend `platform/src/text.rs:65-104`: NFC, trimmed, bounded multiline Note.
fn note(reason: &str) -> String {
    let cleaned: String = reason
        .chars()
        .map(|character| match character {
            '\u{2028}' | '\u{2029}' => '\n',
            other => other,
        })
        .filter(|character| !character.is_control() || matches!(character, '\t' | '\n'))
        .take(MAX_REASON_LENGTH)
        .collect();
    let mut cleaned = match ComposingNormalizerBorrowed::new_nfc().normalize(&cleaned) {
        Cow::Borrowed(_) => cleaned,
        Cow::Owned(normalized) => normalized,
    };
    let end = cleaned.trim_end().len();
    cleaned.truncate(end);
    let start = cleaned.len() - cleaned.trim_start().len();
    drop(cleaned.drain(..start));
    if let Some((end, _character)) = cleaned.char_indices().nth(MAX_REASON_LENGTH) {
        cleaned.truncate(end);
    }
    if cleaned.is_empty() {
        "Brak opisu błędu.".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests;
