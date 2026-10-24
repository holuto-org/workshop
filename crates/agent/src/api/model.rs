//! `/agent` request and response bodies (backend `openapi/agent.json`).
//!
//! Decimals stay strings until the agent converts them, so a malformed amount fails its job with
//! a reason instead of rejecting the whole response.

use jiff::Timestamp;
use posnet::{
    printer::VatRate,
    receipt::{PaymentForm, VAT_SLOTS},
    text::{MAX_LINES, MAX_NAME_LENGTH, printable_characters},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{APP_VERSION, DOCUMENTS_LIMIT, nullable};
use crate::documents::{Document, ReceiptKind};

/// Slots the backend accepts; the printer itself has seven.
const MAX_BACKEND_SLOTS: u8 = 16;

/// Polish VAT rates as `KSeF` codes them; `zw` marks an exempt supply.
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
    /// The code for a printer rate; `None` for a rate Holuto does not use.
    pub fn of(rate: &VatRate) -> Option<Self> {
        if rate.exempt() {
            return Some(Self::Exempt);
        }
        [
            (23, Self::Standard),
            (8, Self::Reduced),
            (5, Self::SuperReduced),
            (0, Self::Zero),
        ]
        .into_iter()
        .find(|(percent, _)| rate.percent == Decimal::from(*percent))
        .map(|(_, code)| code)
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

/// Fresh printer facts sent with every session and heartbeat.
///
/// The backend queues no receipt until it has the limits, which the protocol cannot report, so
/// they are the driver's documented Temo Online values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Telemetry {
    pub serial: String,
    pub model: String,
    pub firmware: String,
    pub app_version: &'static str,
    /// The device's actual slot table, not a set of rates.
    pub vat_slots: Vec<VatSlotRate>,
    pub max_item_name_length: usize,
    pub max_lines_per_receipt: usize,
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
            allowed_characters: printable_characters(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterStatus {
    pub printer: PrinterInfo,
    pub is_online: bool,
    #[serde(deserialize_with = "nullable")]
    pub last_seen_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterInfo {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub name: String,
    #[serde(deserialize_with = "nullable")]
    pub device_serial: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub model: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub firmware: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub app_version: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub max_item_name_length: Option<u32>,
    #[serde(deserialize_with = "nullable")]
    pub max_lines_per_receipt: Option<u32>,
    #[serde(deserialize_with = "nullable")]
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
        if self.max_item_name_length == Some(0) || self.max_lines_per_receipt == Some(0) {
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
    /// A resolution only staff or the backend can make; it is authoritative.
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

/// A fiscal job; its lines, totals and payments come from Holuto and are printed unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: Uuid,
    pub order_id: Uuid,
    pub order_number: i64,
    pub printer_id: Uuid,
    pub state: JobState,
    pub kind: ReceiptKind,
    #[serde(deserialize_with = "nullable")]
    pub allocation_id: Option<Uuid>,
    #[serde(deserialize_with = "nullable")]
    pub buyer_nip: Option<String>,
    pub company_name: String,
    pub total: String,
    pub lines: Vec<JobLine>,
    pub advances: Vec<JobAdvance>,
    pub payments: Vec<JobPayment>,
    /// Immutable once taken; every result names it.
    #[serde(deserialize_with = "nullable")]
    pub attempt: Option<Uuid>,
    #[serde(deserialize_with = "nullable")]
    pub receipt_number: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub result: Option<String>,
    pub created_at: Timestamp,
    #[serde(deserialize_with = "nullable")]
    pub taken_at: Option<Timestamp>,
    #[serde(deserialize_with = "nullable")]
    pub resolved_at: Option<Timestamp>,
}

impl Job {
    /// The shape every job must have before anything acts on it, and that it belongs to the
    /// paired printer.
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
        let slots = self
            .lines
            .iter()
            .map(|line| line.vat_slot)
            .chain(self.advances.iter().map(|advance| advance.vat_slot));
        if slots.into_iter().any(|slot| slot >= MAX_BACKEND_SLOTS) {
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

/// The three results a device can report.
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

/// `done` carries the receipt number and issue time; `failed` and `unknown` leave both null.
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
    pub buyer_nip: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub items: Vec<Document>,
    /// Opaque; `None` on the last page.
    #[serde(deserialize_with = "nullable")]
    pub next_cursor: Option<String>,
}

impl Page {
    /// One bad item rejects the whole page: nothing partial is shown.
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

/// The printer's own slots (A to G) that can carry a Holuto rate.
pub(crate) fn printer_slots(rates: &[VatRate]) -> (Vec<VatSlotRate>, Vec<String>) {
    let mut slots = Vec::new();
    let mut skipped = Vec::new();
    for rate in rates
        .iter()
        .filter(|rate| rate.active() && rate.slot < VAT_SLOTS)
    {
        match VatCode::of(rate) {
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
