//! `/agent` wire bodies, written against the vendored `openapi/agent.json`.
//!
//! Response types read only the fields the client uses and ignore the rest, so a
//! backend addition never breaks a paired printer. Every field they read is required
//! by the contract, and a nullable one must still be present (`Option::deserialize`).
//! Outbound nullable fields serialize explicit nulls. Decimal strings are validated per
//! job at the fiscal boundary, so one malformed job fails alone. Structs never show
//! their values in `Debug`: they carry customer, receipt and device data.
//! `contract::tests` checks these types against the schema.

use std::{fmt, num::NonZeroU32};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::{APP_VERSION, DOCUMENTS_LIMIT, JOBS_LIMIT};
use crate::{
    documents::{Document, ReceiptKind},
    settings::Token,
};
use fiscal::{
    device::{Device, VatKind, VatRate, Vendor},
    receipt::{PaymentForm, VAT_SLOTS},
};
use uuid::Uuid;

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
    pub order_id: Uuid,
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
    pub created_at: Timestamp,
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

/// A job-history row; the order's current names live outside [`Job`].
#[derive(PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub job: Job,
    #[serde(deserialize_with = "Option::deserialize")]
    pub customer_name: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub vehicle_plate: Option<String>,
}

/// A null cursor means its direction has nothing to continue.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub items: Vec<Document>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next_cursor: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub previous_cursor: Option<String>,
}

/// A null cursor means its direction has nothing to continue.
#[derive(PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobsPage {
    pub items: Vec<JobView>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next_cursor: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub previous_cursor: Option<String>,
}

/// `POST …/search` body for documents and job history.
///
/// Criteria may hold personal data, so they travel only in a body. No filter and no
/// sort: the device feed is scoped to its printer and the service orders it.
#[derive(Serialize)]
pub struct TableSearch {
    pub cursor: Cursor,
    pub filter: NoFilter,
    pub search: Search,
}

#[derive(Serialize)]
pub struct Cursor {
    /// Opaque: a page's `nextCursor` or `previousCursor` unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Always sent, so the service's default page size never applies.
    pub limit: NonZeroU32,
}

#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde writes the required `{}` for braces, but `null` for a unit struct"
)]
#[derive(Debug, Default, Serialize)]
pub struct NoFilter {}

#[derive(Serialize)]
pub struct Search {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
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
#[serde(rename_all = "camelCase")]
pub struct QueueForm {
    pub allocation_id: Option<Uuid>,
    pub buyer_nip: Option<String>,
    pub kind: ReceiptKind,
    pub order_id: Uuid,
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
    JobView,
    Page,
    JobsPage,
    TableSearch,
    Cursor,
    Search,
    ResultForm,
    QueueForm,
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

    pub(crate) fn describe(&mut self, device: &Device) -> Vec<String> {
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
    pub(crate) fn check(&self, unique_number: &str) -> Result<(), String> {
        if self
            .unique_number
            .as_deref()
            .is_some_and(|paired| paired != unique_number)
        {
            return Err("sparowano z inną drukarką".to_owned());
        }
        if let Some(c) = &self.capabilities {
            if c.max_quantity_decimals > 8 {
                return Err("nieprawidłowa precyzja ilości drukarki".to_owned());
            }
            if [
                c.max_item_name_length,
                c.max_company_name_length,
                c.max_lines_per_receipt,
            ]
            .contains(&0)
            {
                return Err("zerowy limit drukarki".to_owned());
            }
            if c.allowed_characters.is_empty() {
                return Err("pusta lista znaków drukarki".to_owned());
            }
        }
        if self.vat_rates.len() > usize::from(VAT_SLOTS) {
            return Err("nieprawidłowe litery VAT".to_owned());
        }
        Ok(())
    }
}
impl JobState {
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed | Self::Expired | Self::Voided
        )
    }
}
impl Job {
    /// Holuto's binding of this job to the paired printer.
    pub(crate) fn check_printer(&self, printer_id: Option<Uuid>) -> Result<(), String> {
        if printer_id != Some(self.printer_id) {
            return Err("zadanie innej drukarki".to_owned());
        }
        Ok(())
    }

    /// Job shape; a claimed job that breaks it fails before any fiscal command.
    pub(crate) fn check(&self, max_lines: usize) -> Result<(), String> {
        self.kind.check_allocation(self.allocation_id)?;
        if self.order_number <= 0 {
            return Err("numer zlecenia musi być dodatni".to_owned());
        }
        if self.lines.is_empty() {
            return Err("zadanie musi mieć co najmniej jedną pozycję".to_owned());
        }
        if self.lines.len() > max_lines {
            return Err(format!("zadanie musi mieć od 1 do {max_lines} pozycji"));
        }
        if self.kind == ReceiptKind::Advance && !self.advances.is_empty() {
            return Err("zaliczka nie może odliczać zaliczek".to_owned());
        }
        if self
            .buyer_nip
            .as_deref()
            .is_some_and(|nip| nip.len() != 10 || !nip.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err("nieprawidłowy NIP nabywcy".to_owned());
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
impl Page {
    pub(crate) fn check(&self) -> Result<(), String> {
        if usize::try_from(DOCUMENTS_LIMIT.get()).is_ok_and(|limit| self.items.len() > limit) {
            return Err("strona przekracza limit".to_owned());
        }
        if self.next_cursor.as_deref().is_some_and(str::is_empty) {
            return Err("pusty kursor następnej strony".to_owned());
        }
        if self.previous_cursor.as_deref().is_some_and(str::is_empty) {
            return Err("pusty kursor poprzedniej strony".to_owned());
        }
        self.items.iter().try_for_each(Document::check)
    }
}
impl JobsPage {
    pub(crate) fn check(&self) -> Result<(), String> {
        if usize::try_from(JOBS_LIMIT.get()).is_ok_and(|limit| self.items.len() > limit) {
            return Err("strona przekracza limit".to_owned());
        }
        if [self.next_cursor.as_deref(), self.previous_cursor.as_deref()]
            .into_iter()
            .flatten()
            .any(str::is_empty)
        {
            return Err("pusty kursor strony".to_owned());
        }
        Ok(())
    }
}
pub(crate) fn printer_slots(rates: &[VatRate]) -> (Vec<VatRateMapping>, Vec<String>) {
    let mut mapped = Vec::new();
    let mut skipped = Vec::new();
    for rate in rates.iter().filter(|rate| rate.active()) {
        let Some(letter) = VatLetter::from_slot(rate.slot) else {
            continue;
        };
        match VatCode::of(*rate) {
            Some(vat_rate) => mapped.push(VatRateMapping { letter, vat_rate }),
            None => skipped.push(format!(
                "Pomijam nieobsługiwaną stawkę VAT w slocie {}",
                rate.label()
            )),
        }
    }
    (mapped, skipped)
}
