//! `/agent` bodies from backend `openapi/agent.json`.
//!
//! Decimal strings let malformed amounts fail individual jobs, not entire responses.
//! `Option::deserialize` requires nullable fields to be present; plain `Option` does not.

use fiscal::{
    device::{Device, VatKind, VatRate},
    receipt::VAT_SLOTS,
};
use uuid::Uuid;

use super::{APP_VERSION, DOCUMENTS_LIMIT};
use crate::documents::{Document, ReceiptKind};

type AppVersion = &'static str;

#[expect(
    unused_qualifications,
    unreachable_pub,
    unnameable_types,
    clippy::absolute_paths,
    clippy::renamed_function_params,
    clippy::needless_raw_strings,
    reason = "typify's generated paths, helper visibility and raw doc strings"
)]
mod wire {
    include!(concat!(env!("OUT_DIR"), "/agent-wire.rs"));
}

pub type VatCode = wire::VatCode;
pub type VatSlotRate = wire::VatSlotRate;
pub type Telemetry = wire::Telemetry;
pub type PrinterStatus = wire::PrinterStatus;
pub type PrinterInfo = wire::PrinterInfo;
pub type JobState = wire::JobState;
pub type Job = wire::Job;
pub type JobLine = wire::JobLine;
pub type JobAdvance = wire::JobAdvance;
pub type JobPayment = wire::JobPayment;
pub type Outcome = wire::Outcome;
pub type ResultForm = wire::ResultForm;
pub type QueueForm = wire::QueueForm;
pub type Page = wire::Page;
pub type Pairing = wire::Pairing;
pub type PairForm = wire::PairForm;

/// Backend slot limit; the printer has seven.
const MAX_BACKEND_SLOTS: u8 = 16;

impl VatCode {
    /// `None` for printer rates Holuto does not support.
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
    /// Device identity and limits stay empty until [`Self::describe`] reads them.
    pub fn new(serial: &str) -> Self {
        Self {
            allowed_characters: String::new(),
            app_version: APP_VERSION,
            firmware: String::new(),
            max_company_name_length: 0,
            max_item_name_length: 0,
            max_lines_per_receipt: 0,
            model: String::new(),
            serial: serial.to_owned(),
            vat_slots: Vec::new(),
        }
    }

    /// Takes identity, limits and VAT slots from the connected device; returns notices for
    /// active rates Holuto does not support.
    pub(crate) fn describe(&mut self, device: &Device) -> Vec<String> {
        let (slots, skipped) = printer_slots(&device.vat);
        device.model.clone_into(&mut self.model);
        device.firmware.clone_into(&mut self.firmware);
        let capabilities = &device.capabilities;
        self.max_item_name_length = capabilities.max_item_name;
        self.max_lines_per_receipt = capabilities.max_lines;
        self.max_company_name_length = capabilities.max_company_name;
        capabilities
            .allowed_characters
            .clone_into(&mut self.allowed_characters);
        self.vat_slots = slots;
        skipped
    }
}

impl PrinterInfo {
    pub(crate) fn check(&self, serial: &str) -> Result<(), String> {
        if self
            .device_serial
            .as_deref()
            .is_some_and(|paired| paired != serial)
        {
            return Err("sparowano z inną drukarką".to_owned());
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

impl JobState {
    /// Authoritative staff or backend resolution.
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Expired)
    }
}

impl Job {
    /// `max_lines` is the connected device's limit; paths without a device (recovery) pass
    /// `None` and only require a nonempty job.
    pub(crate) fn check(
        &self,
        printer_id: Option<Uuid>,
        max_lines: Option<usize>,
    ) -> Result<(), String> {
        if printer_id != Some(self.printer_id) {
            return Err("zadanie innej drukarki".to_owned());
        }
        self.kind.check_allocation(self.allocation_id)?;
        if self.order_number <= 0 {
            return Err("numer zlecenia musi być dodatni".to_owned());
        }
        if self.lines.is_empty() {
            return Err("zadanie musi mieć co najmniej jedną pozycję".to_owned());
        }
        if let Some(max) = max_lines
            && self.lines.len() > max
        {
            return Err(format!("zadanie musi mieć od 1 do {max} pozycji"));
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
