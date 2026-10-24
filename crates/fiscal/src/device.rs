//! A connected fiscal printer's identity, readiness, VAT table and the limits its
//! driver advertises.
//!
//! Drivers fill [`Capabilities`]; receipt validation and Holuto telemetry read them, so
//! no vendor's limits live outside its driver crate.

use serde::{Deserialize, Serialize};

use crate::{decimal, error::Error};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vendor {
    Posnet,
    Novitus,
    Elzab,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VatKind {
    Rate {
        /// Hundredths of a percent: `2300` is 23 %.
        basis_points: u32,
    },
    Exempt,
    Inactive,
}

/// One programmed VAT slot of the printer; slot 0 is `A`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VatRate {
    pub slot: u8,
    pub kind: VatKind,
}

impl VatRate {
    pub const fn active(self) -> bool {
        !matches!(self.kind, VatKind::Inactive)
    }

    pub const fn exempt(self) -> bool {
        matches!(self.kind, VatKind::Exempt)
    }

    pub fn label(self) -> String {
        let letter = char::from(b'A'.saturating_add(self.slot));
        let rate = match self.kind {
            VatKind::Exempt => "zwolniona".to_owned(),
            VatKind::Rate { basis_points } => {
                format!("{}%", decimal::format(basis_points.into(), 2)).replace('.', ",")
            },
            VatKind::Inactive => "nieaktywna".to_owned(),
        };
        format!("{letter} · {rate}")
    }
}

/// What a device accepts. Limits are counts of characters (Unicode scalar values).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub max_item_name: usize,
    pub max_lines: usize,
    pub max_company_name: usize,
    pub max_buyer_nip: usize,
    pub max_quantity_decimals: u8,
    /// Every character the device prints unchanged, NFC. Text is never rewritten to fit.
    pub allowed_characters: String,
    pub advance_deductions: bool,
    pub zero_total_receipts: bool,
    pub company_footer: bool,
}

impl Capabilities {
    pub fn allows(&self, text: &str) -> bool {
        text.chars()
            .all(|character| self.allowed_characters.contains(character))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub vendor: Vendor,
    pub model: String,
    pub firmware: String,
    pub serial: String,
    /// Empty when ready.
    pub problems: Vec<String>,
    pub vat: Vec<VatRate>,
    pub capabilities: Capabilities,
}

impl Device {
    pub const fn ready(&self) -> bool {
        self.problems.is_empty()
    }

    pub fn description(&self) -> String {
        if self.ready() {
            "Gotowa do drukowania".to_owned()
        } else {
            self.problems.join(". ")
        }
    }

    /// Refuses while any problem needs operator intervention.
    pub fn require_ready(&self) -> Result<(), Error> {
        if self.ready() {
            Ok(())
        } else {
            Err(Error::NotReady(self.description()))
        }
    }

    /// Refuses a printer other than the one the operation belongs to.
    pub fn require_serial(&self, serial: &str) -> Result<(), Error> {
        if self.serial == serial {
            Ok(())
        } else {
            Err(Error::Invalid(format!(
                "Podłączona drukarka {} nie jest drukarką {serial}, której dotyczy operacja.",
                self.serial
            )))
        }
    }
}

#[cfg(test)]
mod tests;
