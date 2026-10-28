//! `/agent` wire bodies generated from the vendored contract.
//!
//! Decimal strings are validated per job at the fiscal boundary.

use super::{APP_VERSION, DOCUMENTS_LIMIT, JOBS_LIMIT};
use crate::documents::{Document, ReceiptKind};
use fiscal::{
    device::{Device, VatKind, VatRate, Vendor},
    receipt::VAT_SLOTS,
};
use uuid::Uuid;

type AppVersion = &'static str;
#[expect(
    unused_qualifications,
    unreachable_pub,
    unnameable_types,
    clippy::absolute_paths,
    clippy::renamed_function_params,
    clippy::needless_raw_strings,
    reason = "typify generated paths and helper visibility"
)]
mod wire {
    include!(concat!(env!("OUT_DIR"), "/agent-wire.rs"));
}
pub type VatCode = wire::VatCode;
pub type VatLetter = wire::FiscalVatLetter;
pub type VatRateMapping = wire::VatRateMapping;
pub type Telemetry = wire::Telemetry;
pub type PrinterCapabilities = wire::PrinterCapabilities;
pub type PrinterStatus = wire::PrinterStatus;
pub type PrinterInfo = wire::PrinterInfo;
pub type JobState = wire::JobState;
pub type Job = wire::Job;
pub type JobView = wire::JobView;
pub type JobSource = wire::JobSource;
pub type AgentJobFilter = wire::AgentJobFilter;
pub type JobSearch = wire::JobSearch;
pub type JobsSearch = wire::JobsSearch;
pub type JobsPage = wire::JobsPage;
pub type Outcome = wire::Outcome;
pub type ResultForm = wire::ResultForm;
pub type QueueForm = wire::QueueForm;
pub type Page = wire::Page;
pub type DocumentsSearch = wire::DocumentsSearch;
pub type DocumentsCursor = wire::DocumentsCursor;
pub type DocumentsQuery = wire::DocumentsQuery;
pub type Pairing = wire::Pairing;
pub type PairForm = wire::PairForm;

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
            VatKind::Exempt => Some(Self::Zw),
            VatKind::Rate { basis_points } => match basis_points {
                2300 => Some(Self::X23),
                800 => Some(Self::X8),
                500 => Some(Self::X5),
                0 => Some(Self::X0),
                _ => None,
            },
            VatKind::Inactive => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::X23 => "23%",
            Self::X8 => "8%",
            Self::X5 => "5%",
            Self::X0 => "0%",
            Self::Zw => "zw",
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
