//! A connected fiscal printer's identity, readiness, VAT table and the limits its
//! driver advertises.
//!
//! Drivers fill [`Capabilities`]; receipt validation and Holuto telemetry read them, so
//! no vendor's limits live outside its driver crate.

use serde::{Deserialize, Serialize};

use crate::{decimal, error::Error};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub const fn legal_letter(self) -> bool {
        if !self.active() {
            return true;
        }
        match self.slot {
            0 => matches!(self.kind, VatKind::Rate { basis_points: 2300 }),
            1 => matches!(self.kind, VatKind::Rate { basis_points: 800 }),
            2 => matches!(self.kind, VatKind::Rate { basis_points: 500 }),
            3 => matches!(self.kind, VatKind::Rate { basis_points: 0 }),
            4 => matches!(self.kind, VatKind::Exempt),
            5 | 6 => true,
            _ => false,
        }
    }

    pub const fn active(self) -> bool {
        !matches!(self.kind, VatKind::Inactive)
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
/// The default accepts nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

/// Readiness failures shared by drivers, independent of vendor status bits.
#[derive(Clone, Debug)]
pub enum Problem {
    Blocking(String),
    DailyReportOverdue,
    /// Sales and daily reports are refused. Monthly and periodic reports print only when
    /// `periodic_reports` is set, i.e. the driver's specification permits them in this mode.
    ReadOnly { periodic_reports: bool },
}

impl Problem {
    fn description(&self) -> &str {
        match self {
            Self::Blocking(message) => message,
            Self::DailyReportOverdue => "Wymagany zaległy raport dobowy",
            Self::ReadOnly { .. } => "Drukarka jest w trybie tylko do odczytu",
        }
    }
}

impl From<&str> for Problem {
    fn from(value: &str) -> Self {
        Self::Blocking(value.to_owned())
    }
}

#[derive(Clone, Debug)]
pub struct Device {
    pub vendor: Vendor,
    pub model: String,
    pub firmware: String,
    pub unique_number: String,
    /// Empty when ready.
    pub problems: Vec<Problem>,
    pub vat: Vec<VatRate>,
    pub capabilities: Capabilities,
}

impl Device {
    /// `None` when the driver cannot truthfully advertise the backend's required limits.
    /// Readiness problems remain independent: paper-out devices may still be describable.
    pub fn describable_capabilities(&self) -> Option<&Capabilities> {
        let capabilities = &self.capabilities;
        (valid_unique_number(&self.unique_number)
            && self.vat.iter().all(|r| r.legal_letter())
            && capabilities.max_item_name > 0
            && capabilities.max_lines > 0
            && capabilities.max_company_name > 0
            && capabilities.max_quantity_decimals <= 8
            && !capabilities.allowed_characters.is_empty())
        .then_some(capabilities)
    }

    pub fn ready(&self) -> bool {
        self.problems.is_empty()
            && valid_unique_number(&self.unique_number)
            && self.vat.iter().all(|r| r.legal_letter())
    }

    /// UI eligibility only; the driver must still run operation-specific preflight.
    pub fn daily_report_allowed(&self) -> bool {
        self.ready_except(|problem| matches!(problem, Problem::DailyReportOverdue))
    }

    /// UI eligibility for monthly and periodic reports, which some read-only devices still print.
    pub fn periodic_reports_allowed(&self) -> bool {
        self.ready_except(|problem| {
            matches!(problem, Problem::ReadOnly {
                periodic_reports: true
            })
        })
    }

    fn ready_except(&self, tolerated: impl Fn(&Problem) -> bool) -> bool {
        valid_unique_number(&self.unique_number)
            && self.vat.iter().all(|rate| rate.legal_letter())
            && self.problems.iter().all(tolerated)
    }

    pub fn description(&self) -> String {
        if self.ready() {
            "Gotowa do drukowania".to_owned()
        } else if !valid_unique_number(&self.unique_number) {
            "Nieprawidłowy numer unikatowy drukarki".to_owned()
        } else if !self.vat.iter().all(|r| r.legal_letter()) {
            "Nieprawidłowe przypisanie liter VAT A–E".to_owned()
        } else {
            let mut description = String::new();
            for problem in &self.problems {
                if !description.is_empty() {
                    description.push_str(". ");
                }
                description.push_str(problem.description());
            }
            description
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

    /// Refuses monthly and periodic reports unless read-only mode that permits them is the
    /// only problem.
    pub fn require_periodic_reports(&self) -> Result<(), Error> {
        if self.periodic_reports_allowed() {
            Ok(())
        } else {
            Err(Error::NotReady(self.description()))
        }
    }

    /// Resolving an interrupted operation tolerates an overdue daily report, which may be
    /// the interrupted one and cannot run until resolution, and read-only mode, which
    /// would otherwise block resolution indefinitely. Resolution never prints.
    pub fn require_resolvable(&self) -> Result<(), Error> {
        if self.ready_except(|problem| {
            matches!(problem, Problem::DailyReportOverdue | Problem::ReadOnly { .. })
        }) {
            Ok(())
        } else {
            Err(Error::NotReady(self.description()))
        }
    }

    /// Refuses a printer other than the one the operation belongs to.
    pub fn require_unique_number(&self, unique_number: &str) -> Result<(), Error> {
        if self.unique_number == unique_number {
            Ok(())
        } else {
            Err(Error::Invalid(format!(
                "Podłączona drukarka {} nie jest drukarką {unique_number}, \
                 której dotyczy operacja.",
                self.unique_number
            )))
        }
    }
}

#[cfg(test)]
mod tests;

pub fn valid_unique_number(value: &str) -> bool {
    value.len() == 13
        && value.bytes().enumerate().all(|(index, byte)| {
            if index < 3 {
                byte.is_ascii_uppercase()
            } else {
                byte.is_ascii_digit()
            }
        })
}
