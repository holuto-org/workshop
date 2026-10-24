//! Fiscal report requests; their serde representation is persisted in the operation journal.

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    /// Closes the current day in fiscal memory; the date is the printer's own.
    Daily,
    Monthly {
        year: i16,
        month: i8,
    },
    Periodic {
        from: Date,
        to: Date,
        summary: bool,
    },
}

impl Report {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Daily => "Raport dobowy",
            Self::Monthly { .. } => "Raport miesięczny",
            Self::Periodic { .. } => "Raport okresowy",
        }
    }
}

#[cfg(test)]
mod tests;
