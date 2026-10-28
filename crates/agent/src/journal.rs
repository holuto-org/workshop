//! Persisted before each printer command and retained until settlement.
//! An unreadable journal blocks printing.
//!
//! Nullable keys are required: the journal always writes them explicitly.
//! API text is canonicalized without replacing the raw completed-device evidence.

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use std::{borrow::Cow, fmt, io, marker::PhantomData, path::PathBuf};

use fiscal::{
    device::Vendor,
    money::Grosze,
    receipt::{Payment, Receipt},
    report::Report,
};
use icu_normalizer::ComposingNormalizerBorrowed;
use icu_properties::{
    CodePointSetData,
    props::{
        DefaultIgnorableCodePoint, EmojiComponent, JoinControl, NoncharacterCodePoint,
        VariationSelector,
    },
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use i18n::Message;

use crate::{
    Error,
    api::model::{JobState, Outcome, ResultForm},
    store::{self, StateDir},
};

const FILE: &str = "operation.json";
/// API `Note` limit.
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
    Completed {
        #[serde(deserialize_with = "Option::deserialize")]
        receipt_number: Option<String>,
    },
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
    pub(crate) device_unique_number: String,
    pub(crate) device: Device,
    #[serde(deserialize_with = "Option::deserialize")]
    pub(crate) summary: Option<Summary>,
    /// Persisted before sending; resent unchanged until accepted.
    #[serde(deserialize_with = "Option::deserialize")]
    pub(crate) result: Option<JobResult>,
    #[serde(deserialize_with = "Option::deserialize")]
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
    /// An unreadable receipt number stays UNKNOWN until staff supply it.
    pub(crate) fn done(receipt_number: Option<String>) -> Self {
        let Some(raw) = receipt_number else {
            return Self::unknown(&Message::new("agent-unnumbered").to_string());
        };
        receipt_code(&raw).map_or_else(
            || Self::unknown(&Message::new("agent-invalid-receipt-number").to_string()),
            |receipt_number| Self::Done { receipt_number },
        )
    }

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
    pub(crate) device_unique_number: String,
    pub(crate) started_at: Timestamp,
    pub(crate) device: Device,
    /// Operator-confirmed printout on the same ready device.
    #[serde(deserialize_with = "Option::deserialize")]
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

pub(crate) struct Journal<'dir> {
    path: PathBuf,
    _lock: PhantomData<&'dir StateDir>,
    /// Writes until an injected failure, plus one; zero disarms.
    #[cfg(test)]
    fail_write: AtomicUsize,
}

impl<'dir> Journal<'dir> {
    pub(crate) fn new(dir: &'dir StateDir) -> Self {
        Self {
            path: dir.file(FILE),
            _lock: PhantomData,
            #[cfg(test)]
            fail_write: AtomicUsize::new(0),
        }
    }

    /// Never writes; an unreadable or invalid journal blocks.
    pub(crate) fn load(&self) -> Result<Option<Operation>, Error> {
        let bytes = store::read_optional(&self.path).map_err(|source| Error::Store {
            action: Message::new("agent-read-journal"),
            path: self.path.clone(),
            source,
        })?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_invalid_json| Error::Blocked(Message::new("agent-bad-journal")))
    }

    pub(crate) fn save(&self, operation: &Operation) -> Result<(), Error> {
        self.write(operation).map_err(|source| Error::Store {
            action: Message::new("agent-write-journal"),
            path: self.path.clone(),
            source,
        })
    }

    /// Lets `successes` writes through, then fails the next one once.
    #[cfg(test)]
    pub(crate) fn fail_write_after(&self, successes: usize) {
        self.fail_write.store(successes + 1, Ordering::SeqCst);
    }

    pub(crate) fn write(&self, operation: &Operation) -> io::Result<()> {
        #[cfg(test)]
        if self
            .fail_write
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            == Ok(1)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected journal write failure",
            ));
        }
        let bytes = serde_json::to_vec_pretty(operation).map_err(io::Error::other)?;
        store::write_durably(&self.path, &bytes)
    }
}

/// API `Code` rules; raw printer evidence remains in `Device::Completed`.
fn receipt_code(raw: &str) -> Option<String> {
    const MAX_LENGTH: usize = 64;
    if raw.chars().nth(MAX_LENGTH).is_some() {
        return None;
    }
    let normalized = ComposingNormalizerBorrowed::new_nfkc().normalize(raw);
    if normalized.contains(['\u{2028}', '\u{2029}']) {
        return None;
    }
    let trimmed = normalized.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_control() || invisible(c)) {
        return None;
    }
    let mut code = String::with_capacity(trimmed.len());
    for word in trimmed.split_whitespace() {
        if !code.is_empty() {
            code.push(' ');
        }
        code.push_str(word);
    }
    (code.chars().nth(MAX_LENGTH).is_none()).then_some(code)
}

/// RFC 8264 ignorable properties, preserving joining scripts and emoji.
fn invisible(c: char) -> bool {
    (CodePointSetData::new::<DefaultIgnorableCodePoint>().contains(c)
        || CodePointSetData::new::<NoncharacterCodePoint>().contains(c))
        && !CodePointSetData::new::<JoinControl>().contains(c)
        && !CodePointSetData::new::<VariationSelector>().contains(c)
        && !CodePointSetData::new::<EmojiComponent>().contains(c)
}

/// API `Note` rules: NFC, trimmed, bounded multiline text.
fn note(reason: &str) -> String {
    let cleaned: String = reason
        .chars()
        .map(|character| match character {
            '\u{2028}' | '\u{2029}' => '\n',
            other => other,
        })
        .filter(|character| {
            (!character.is_control() || matches!(character, '\t' | '\n')) && !invisible(*character)
        })
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
        Message::new("agent-no-error-description").to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests;
