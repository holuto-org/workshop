//! `/agent` wire bodies, written against the vendored `openapi/agent.json`.
//!
//! Response types read only the fields the client uses and ignore the rest, so a
//! backend addition never breaks a paired printer. Every field they read is required
//! by the contract, and a nullable one must still be present (`Option::deserialize`).
//! Outbound nullable fields serialize explicit nulls. Decimal strings are validated per
//! job at the fiscal boundary, so one malformed job fails alone. Structs never show
//! their values in `Debug`: they carry customer, receipt and device data.
//! `contract::tests` checks these types against the schema.

use std::fmt;

use i18n::Message;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::APP_VERSION;
use crate::settings::Token;
use fiscal::{
    device::{Device, VatKind, VatRate, Vendor},
    receipt::{PaymentForm, VAT_SLOTS},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReceiptKind {
    /// Payment allocation with one line per VAT rate.
    Advance,
    /// Posted order's final sale; unpaid balance prints as credit.
    Sale,
}

impl ReceiptKind {
    pub(crate) const fn check_allocation(self, allocation_id: Option<Uuid>) -> Result<(), Message> {
        match (self, allocation_id) {
            (Self::Advance, Some(_)) | (Self::Sale, None) => Ok(()),
            (Self::Advance, None) => Err(Message::new("agent-allocation-missing")),
            (Self::Sale, Some(_)) => Err(Message::new("agent-allocation-sale")),
        }
    }
}

/// Polish VAT rates as `KSeF` codes them; `zw` marks an exempt supply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VatCode {
    #[serde(rename = "23")]
    Rate23,
    #[serde(rename = "8")]
    Rate8,
    #[serde(rename = "5")]
    Rate5,
    #[serde(rename = "0")]
    Rate0,
    #[serde(rename = "zw")]
    Exempt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VatLetter {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
}

/// Also the journal's stored settlement state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Taken,
    Done,
    Failed,
    Expired,
    Unknown,
    Voided,
}

/// A reported attempt's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Done,
    Failed,
    Unknown,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VatRateMapping {
    pub letter: VatLetter,
    pub vat_rate: VatCode,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterCapabilities {
    pub advance_deductions: bool,
    pub allowed_characters: String,
    pub max_company_name_length: usize,
    pub max_item_name_length: usize,
    pub max_lines_per_receipt: usize,
    pub max_quantity_decimals: u8,
    pub zero_total_receipts: bool,
}

/// `POST /agent/session` body: what the connected printer reports.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Telemetry {
    pub app_version: &'static str,
    pub capabilities: PrinterCapabilities,
    pub firmware: String,
    pub model: String,
    pub unique_number: String,
    pub vat_rates: Vec<VatRateMapping>,
    pub vendor: Vendor,
}

#[derive(Deserialize)]
pub struct PrinterStatus {
    pub printer: PrinterInfo,
}

/// The paired printer as Holuto last recorded it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterInfo {
    pub id: Uuid,
    #[serde(deserialize_with = "Option::deserialize")]
    pub unique_number: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub capabilities: Option<PrinterCapabilities>,
    pub vat_rates: Vec<VatRateMapping>,
}

#[derive(PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: Uuid,
    pub printer_id: Uuid,
    pub kind: ReceiptKind,
    pub order_number: i64,
    #[serde(deserialize_with = "Option::deserialize")]
    pub allocation_id: Option<Uuid>,
    pub company_name: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub buyer_nip: Option<String>,
    pub lines: Vec<JobLine>,
    pub advances: Vec<JobAdvance>,
    pub payments: Vec<JobPayment>,
    pub total: String,
    pub state: JobState,
    #[serde(deserialize_with = "Option::deserialize")]
    pub attempt: Option<Uuid>,
    /// The printer's issue time of the job's receipt.
    #[serde(deserialize_with = "Option::deserialize")]
    pub issued_at: Option<Timestamp>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub receipt_number: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub result: Option<String>,
}

#[derive(PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLine {
    pub name: String,
    pub quantity: String,
    pub unit_price: String,
    pub total: String,
    pub vat_letter: VatLetter,
    pub vat_rate: VatCode,
}

#[derive(PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAdvance {
    pub amount: String,
    pub vat_letter: VatLetter,
    pub vat_rate: VatCode,
}

#[derive(PartialEq, Eq, Deserialize)]
pub struct JobPayment {
    pub amount: String,
    pub form: PaymentForm,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultForm {
    pub attempt: Uuid,
    pub issued_at: Option<Timestamp>,
    pub receipt_number: Option<String>,
    pub result: Option<String>,
    pub state: Outcome,
}

#[derive(Serialize)]
pub struct PairForm {
    pub code: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub printer_id: Uuid,
    pub token: Token,
}

macro_rules! redacted_debug {
    ($($name:ident),+ $(,)?) => {$(
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name)).finish_non_exhaustive()
            }
        }
    )+};
}

redacted_debug!(
    VatRateMapping,
    PrinterCapabilities,
    Telemetry,
    PrinterStatus,
    PrinterInfo,
    Job,
    JobLine,
    JobAdvance,
    JobPayment,
    ResultForm,
    PairForm,
    Pairing,
);

impl VatLetter {
    pub const fn slot(self) -> u8 {
        match self {
            Self::A => 0,
            Self::B => 1,
            Self::C => 2,
            Self::D => 3,
            Self::E => 4,
            Self::F => 5,
            Self::G => 6,
        }
    }

    /// A–G are slots 0–6; every other slot is outside the wire contract.
    pub const fn from_slot(slot: u8) -> Option<Self> {
        match slot {
            0 => Some(Self::A),
            1 => Some(Self::B),
            2 => Some(Self::C),
            3 => Some(Self::D),
            4 => Some(Self::E),
            5 => Some(Self::F),
            6 => Some(Self::G),
            _ => None,
        }
    }
}
impl VatCode {
    pub const fn of(rate: VatRate) -> Option<Self> {
        match rate.kind {
            VatKind::Exempt => Some(Self::Exempt),
            VatKind::Rate { basis_points } => match basis_points {
                2300 => Some(Self::Rate23),
                800 => Some(Self::Rate8),
                500 => Some(Self::Rate5),
                0 => Some(Self::Rate0),
                _ => None,
            },
            VatKind::Inactive => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Rate23 => "23%",
            Self::Rate8 => "8%",
            Self::Rate5 => "5%",
            Self::Rate0 => "0%",
            Self::Exempt => "zw",
        }
    }
}
impl Telemetry {
    pub fn new(unique_number: &str, vendor: Vendor) -> Self {
        Self {
            app_version: APP_VERSION,
            capabilities: PrinterCapabilities {
                advance_deductions: false,
                allowed_characters: String::new(),
                max_company_name_length: 0,
                max_item_name_length: 0,
                max_lines_per_receipt: 0,
                max_quantity_decimals: 0,
                zero_total_receipts: false,
            },
            firmware: String::new(),
            model: String::new(),
            unique_number: unique_number.to_owned(),
            vat_rates: Vec::new(),
            vendor,
        }
    }

    pub(crate) fn describe(&mut self, device: &Device) -> Vec<Message> {
        let (rates, skipped) = printer_slots(&device.vat);
        self.vendor = device.vendor;
        device.model.clone_into(&mut self.model);
        device.firmware.clone_into(&mut self.firmware);
        let c = &device.capabilities;
        self.capabilities = PrinterCapabilities {
            advance_deductions: c.advance_deductions,
            allowed_characters: c.allowed_characters.clone(),
            max_company_name_length: c.max_company_name,
            max_item_name_length: c.max_item_name,
            max_lines_per_receipt: c.max_lines,
            max_quantity_decimals: c.max_quantity_decimals,
            zero_total_receipts: c.zero_total_receipts,
        };
        self.vat_rates = rates;
        skipped
    }
}
impl PrinterInfo {
    pub(crate) fn check(&self, unique_number: &str) -> Result<(), Message> {
        if self
            .unique_number
            .as_deref()
            .is_some_and(|paired| paired != unique_number)
        {
            return Err(Message::new("agent-paired-other"));
        }
        if let Some(c) = &self.capabilities {
            if c.max_quantity_decimals > 8 {
                return Err(Message::new("agent-invalid-precision"));
            }
            if [
                c.max_item_name_length,
                c.max_company_name_length,
                c.max_lines_per_receipt,
            ]
            .contains(&0)
            {
                return Err(Message::new("agent-zero-limit"));
            }
            if c.allowed_characters.is_empty() {
                return Err(Message::new("agent-empty-characters"));
            }
        }
        if self.vat_rates.len() > usize::from(VAT_SLOTS) {
            return Err(Message::new("agent-invalid-vat-letters"));
        }
        Ok(())
    }
}
impl JobState {
    pub(crate) const fn message(self) -> Message {
        Message::new(match self {
            Self::Queued => "agent-state-queued",
            Self::Taken => "agent-state-taken",
            Self::Done => "agent-state-done",
            Self::Failed => "agent-state-failed",
            Self::Expired => "agent-state-expired",
            Self::Unknown => "agent-state-unknown",
            Self::Voided => "agent-state-voided",
        })
    }

    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed | Self::Expired | Self::Voided
        )
    }
}
impl Job {
    /// Holuto's binding of this job to the paired printer.
    pub(crate) fn check_printer(&self, printer_id: Option<Uuid>) -> Result<(), Message> {
        if printer_id != Some(self.printer_id) {
            return Err(Message::new("agent-job-other-printer"));
        }
        Ok(())
    }

    /// Job shape; a claimed job that breaks it fails before any fiscal command.
    pub(crate) fn check(&self, max_lines: usize) -> Result<(), Message> {
        self.kind.check_allocation(self.allocation_id)?;
        if self.order_number <= 0 {
            return Err(Message::new("agent-order-positive"));
        }
        if self.lines.is_empty() {
            return Err(Message::new("agent-job-empty"));
        }
        if self.lines.len() > max_lines {
            return Err(Message::new("agent-job-lines").arg("max", max_lines));
        }
        if self.kind == ReceiptKind::Advance && !self.advances.is_empty() {
            return Err(Message::new("agent-advance-deducts"));
        }
        if self
            .buyer_nip
            .as_deref()
            .is_some_and(|nip| nip.len() != 10 || !nip.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(Message::new("agent-invalid-nip"));
        }
        Ok(())
    }
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
pub(crate) fn printer_slots(rates: &[VatRate]) -> (Vec<VatRateMapping>, Vec<Message>) {
    let mut mapped = Vec::new();
    let mut skipped = Vec::new();
    for rate in rates.iter().filter(|rate| rate.active()) {
        let Some(letter) = VatLetter::from_slot(rate.slot) else {
            continue;
        };
        match VatCode::of(*rate) {
            Some(vat_rate) => mapped.push(VatRateMapping { letter, vat_rate }),
            None => skipped.push(Message::new("agent-skipped-vat").arg("slot", rate.message())),
        }
    }
    (mapped, skipped)
}
