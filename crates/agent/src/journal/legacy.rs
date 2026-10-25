//! Only the complete pre-vendor schema can acquire the historical Posnet identity.

use fiscal::{device::Vendor, report::Report};
use jiff::Timestamp;
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
    summary: Option<Summary>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    result: Option<JobResult>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    settled: Option<Settlement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReport {
    report: Report,
    device_serial: String,
    started_at: Timestamp,
    device: Device,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    acknowledged_at: Option<Timestamp>,
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
            summary: old.summary,
            result: old.result,
            settled: old.settled,
        }),
        LegacyOperation::Report(old) => Operation::Report(ReportOperation {
            report: old.report,
            vendor: Vendor::Posnet,
            device_serial: old.device_serial,
            started_at: old.started_at,
            device: old.device,
            acknowledged_at: old.acknowledged_at,
        }),
    })
}
