//! `/agent` bodies from backend `openapi/agent.json`.
//! Decimal strings let malformed amounts fail individual jobs, not entire responses.
//! `Option::deserialize` requires nullable fields to be present; plain `Option` does not.

use jiff::Timestamp;
use posnet::{
    printer::VatRate,
    receipt::{PaymentForm, VAT_SLOTS},
    text::{MAX_COMPANY_NAME_LENGTH, MAX_LINES, MAX_NAME_LENGTH, printable_characters},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{APP_VERSION, DOCUMENTS_LIMIT};
use crate::documents::{BuyerNip, Document, ReceiptKind};

/// Backend slot limit; the printer has seven.
const MAX_BACKEND_SLOTS: u8 = 16;

/// `KSeF` VAT codes; `zw` means exempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VatCode {
    #[serde(rename = "23")]
    Standard,
    #[serde(rename = "8")]
    Reduced,
    #[serde(rename = "5")]
    SuperReduced,
    #[serde(rename = "0")]
    Zero,
    #[serde(rename = "zw")]
    Exempt,
}

impl VatCode {
    /// `None` for printer rates Holuto does not support.
    pub const fn of(rate: VatRate) -> Option<Self> {
        if rate.exempt() {
            return Some(Self::Exempt);
        }
        match rate.basis_points {
            2300 => Some(Self::Standard),
            800 => Some(Self::Reduced),
            500 => Some(Self::SuperReduced),
            0 => Some(Self::Zero),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Standard => "23%",
            Self::Reduced => "8%",
            Self::SuperReduced => "5%",
            Self::Zero => "0%",
            Self::Exempt => "zw",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VatSlotRate {
    pub slot: u8,
    pub vat_rate: VatCode,
}

/// Session and heartbeat telemetry. The backend requires limits before queueing receipts.
/// Limits use documented Temo Online values because the protocol cannot report them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Telemetry {
    pub serial: String,
    pub model: String,
    pub firmware: String,
    pub app_version: &'static str,
    /// Actual slot assignments, not a set of rates.
    pub vat_slots: Vec<VatSlotRate>,
    pub max_item_name_length: usize,
    pub max_lines_per_receipt: usize,
    pub max_company_name_length: usize,
    pub allowed_characters: String,
}

impl Telemetry {
    pub fn new(serial: &str) -> Self {
        Self {
            serial: serial.to_owned(),
            model: String::new(),
            firmware: String::new(),
            app_version: APP_VERSION,
            vat_slots: Vec::new(),
            max_item_name_length: MAX_NAME_LENGTH,
            max_lines_per_receipt: MAX_LINES,
            max_company_name_length: MAX_COMPANY_NAME_LENGTH,
            allowed_characters: printable_characters(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterStatus {
    pub printer: PrinterInfo,
    pub is_online: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub last_seen_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterInfo {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub name: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub device_serial: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub model: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub firmware: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub app_version: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub max_item_name_length: Option<u32>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub max_lines_per_receipt: Option<u32>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub max_company_name_length: Option<u32>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub allowed_characters: Option<String>,
    pub vat_slots: Vec<VatSlotRate>,
}

impl PrinterInfo {
    pub(crate) fn check(&self, serial: &str) -> Result<(), String> {
        if self
            .device_serial
            .as_deref()
            .is_some_and(|paired| paired != serial)
        {
            return Err("token sparowany z inną drukarką".to_owned());
        }
        if [
            self.max_item_name_length,
            self.max_lines_per_receipt,
            self.max_company_name_length,
        ]
        .contains(&Some(0))
        {
            return Err("zerowy limit drukarki".to_owned());
        }
        if self
            .allowed_characters
            .as_deref()
            .is_some_and(str::is_empty)
        {
            return Err("pusta lista znaków drukarki".to_owned());
        }
        if self.vat_slots.len() > usize::from(MAX_BACKEND_SLOTS)
            || self
                .vat_slots
                .iter()
                .any(|slot| slot.slot >= MAX_BACKEND_SLOTS)
        {
            return Err("nieprawidłowe sloty VAT".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Taken,
    Done,
    Failed,
    Expired,
    Unknown,
}

impl JobState {
    /// Authoritative staff or backend resolution.
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Expired)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLine {
    pub name: String,
    pub quantity: String,
    pub unit_price: String,
    pub total: String,
    pub vat_rate: VatCode,
    pub vat_slot: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAdvance {
    pub vat_slot: u8,
    pub vat_rate: VatCode,
    pub amount: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobPayment {
    pub form: PaymentForm,
    pub amount: String,
}

/// Holuto supplies the lines, totals and payments; printing must not alter them.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: Uuid,
    pub order_id: Uuid,
    pub order_number: i64,
    pub printer_id: Uuid,
    pub state: JobState,
    pub kind: ReceiptKind,
    #[serde(deserialize_with = "Option::deserialize")]
    pub allocation_id: Option<Uuid>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub buyer_nip: Option<String>,
    pub company_name: String,
    pub total: String,
    pub lines: Vec<JobLine>,
    pub advances: Vec<JobAdvance>,
    pub payments: Vec<JobPayment>,
    /// Immutable after claim; required in every result.
    #[serde(deserialize_with = "Option::deserialize")]
    pub attempt: Option<Uuid>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub receipt_number: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub result: Option<String>,
    pub created_at: Timestamp,
    #[serde(deserialize_with = "Option::deserialize")]
    pub taken_at: Option<Timestamp>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub resolved_at: Option<Timestamp>,
}

impl Job {
    pub(crate) fn check(&self, printer_id: Option<Uuid>) -> Result<(), String> {
        if printer_id != Some(self.printer_id) {
            return Err("zadanie innej drukarki".to_owned());
        }
        self.kind.check_allocation(self.allocation_id)?;
        if self.order_number <= 0 {
            return Err("numer zlecenia musi być dodatni".to_owned());
        }
        if !(1..=MAX_LINES).contains(&self.lines.len()) {
            return Err("zadanie musi mieć od 1 do 500 pozycji".to_owned());
        }
        if self.kind == ReceiptKind::Advance && !self.advances.is_empty() {
            return Err("zaliczka nie może odliczać zaliczek".to_owned());
        }
        let mut slots = self
            .lines
            .iter()
            .map(|line| line.vat_slot)
            .chain(self.advances.iter().map(|advance| advance.vat_slot));
        if slots.any(|slot| slot >= MAX_BACKEND_SLOTS) {
            return Err("nieprawidłowy slot VAT".to_owned());
        }
        if self
            .buyer_nip
            .as_deref()
            .is_some_and(|nip| nip.len() != 10 || !nip.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err("nieprawidłowy NIP nabywcy".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Done,
    Failed,
    Unknown,
}

impl From<Outcome> for JobState {
    fn from(value: Outcome) -> Self {
        match value {
            Outcome::Done => Self::Done,
            Outcome::Failed => Self::Failed,
            Outcome::Unknown => Self::Unknown,
        }
    }
}

/// Only `done` carries a receipt number and issue time; otherwise both are null.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultForm {
    pub attempt: Uuid,
    pub state: Outcome,
    pub receipt_number: Option<String>,
    pub issued_at: Option<Timestamp>,
    pub result: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueForm {
    pub kind: ReceiptKind,
    pub order_id: Uuid,
    pub allocation_id: Option<Uuid>,
    pub buyer_nip: Option<BuyerNip>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub items: Vec<Document>,
    /// Opaque; `None` on the last page.
    #[serde(deserialize_with = "Option::deserialize")]
    pub next_cursor: Option<String>,
}

impl Page {
    /// Rejects the whole page if any item is invalid.
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.items.len() > DOCUMENTS_LIMIT {
            return Err("strona przekracza limit".to_owned());
        }
        if self.next_cursor.as_deref().is_some_and(str::is_empty) {
            return Err("pusty kursor następnej strony".to_owned());
        }
        self.items.iter().try_for_each(Document::check)
    }
}

pub(crate) fn printer_slots(rates: &[VatRate]) -> (Vec<VatSlotRate>, Vec<String>) {
    let mut slots = Vec::new();
    let mut skipped = Vec::new();
    for rate in rates
        .iter()
        .filter(|rate| rate.active() && rate.slot < VAT_SLOTS)
    {
        match VatCode::of(*rate) {
            Some(vat_rate) => slots.push(VatSlotRate {
                slot: rate.slot,
                vat_rate,
            }),
            None => skipped.push(format!(
                "Pomijam nieobsługiwaną stawkę VAT w slocie {}",
                rate.label()
            )),
        }
    }
    (slots, skipped)
}
