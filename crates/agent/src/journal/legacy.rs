//! Only the complete pre-vendor schema can acquire the historical Posnet identity.

use fiscal::{
    device::Vendor,
    money::Grosze,
    receipt::{Payment, PaymentForm},
    report::Report,
};
use jiff::{Timestamp, civil::Date};
use serde::Deserialize;
use uuid::Uuid;

use super::{Device, JobResult, Operation, ReceiptOperation, ReportOperation, Settlement, Summary};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum LegacyOperation {
    Receipt(LegacyReceipt),
    Report(LegacyReport),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReceipt {
    job_id: Uuid,
    attempt: Uuid,
    issued_at: Timestamp,
    device_serial: String,
    device: Device,
    // Missing evidence fields are corruption, not an older supported schema.
    #[serde(deserialize_with = "Deserialize::deserialize")]
    summary: Option<LegacySummary>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    result: Option<JobResult>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    settled: Option<Settlement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReport {
    report: LegacyReportRequest,
    device_serial: String,
    started_at: Timestamp,
    device: Device,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    acknowledged_at: Option<Timestamp>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySummary {
    total: Grosze,
    payments: Vec<LegacyPayment>,
    items: Vec<String>,
}

impl LegacySummary {
    fn into_current(self) -> Summary {
        Summary {
            total: self.total,
            payments: self.payments.into_iter().map(Payment::from).collect(),
            items: self.items,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyPayment {
    form: PaymentForm,
    amount: Grosze,
}

impl From<LegacyPayment> for Payment {
    fn from(value: LegacyPayment) -> Self {
        Self {
            form: value.form,
            amount: value.amount,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LegacyReportRequest {
    // A struct variant enforces unknown-field rejection; an internally tagged unit does not.
    Daily {},
    Monthly { year: i16, month: i8 },
    Periodic { from: Date, to: Date, summary: bool },
}

impl From<LegacyReportRequest> for Report {
    fn from(value: LegacyReportRequest) -> Self {
        match value {
            LegacyReportRequest::Daily {} => Self::Daily,
            LegacyReportRequest::Monthly { year, month } => Self::Monthly { year, month },
            LegacyReportRequest::Periodic { from, to, summary } => {
                Self::Periodic { from, to, summary }
            },
        }
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<Operation, serde_json::Error> {
    Ok(match serde_json::from_slice(bytes)? {
        LegacyOperation::Receipt(old) => Operation::Receipt(ReceiptOperation {
            job_id: old.job_id,
            attempt: old.attempt,
            issued_at: old.issued_at,
            vendor: Vendor::Posnet,
            device_serial: old.device_serial,
            device: old.device,
            summary: old.summary.map(LegacySummary::into_current),
            result: old.result,
            settled: old.settled,
        }),
        LegacyOperation::Report(old) => Operation::Report(ReportOperation {
            report: old.report.into(),
            vendor: Vendor::Posnet,
            device_serial: old.device_serial,
            started_at: old.started_at,
            device: old.device,
            acknowledged_at: old.acknowledged_at,
        }),
    })
}
