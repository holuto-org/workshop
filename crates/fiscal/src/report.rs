//! Fiscal report requests; their serde representation is persisted in the operation journal.

use jiff::civil::Date;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Report {
    /// Closes the current day in fiscal memory; the date is the printer's own.
    #[serde(deserialize_with = "daily_fields")]
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
    pub const fn message(self) -> i18n::Message {
        i18n::Message::new(match self {
            Self::Daily => "fiscal-report-daily",
            Self::Monthly { .. } => "fiscal-report-monthly",
            Self::Periodic { .. } => "fiscal-report-periodic",
        })
    }

    /// Stable Polish label for journal and CLI diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Daily => "Raport dobowy",
            Self::Monthly { .. } => "Raport miesięczny",
            Self::Periodic { .. } => "Raport okresowy",
        }
    }
}

// Serde's internally tagged unit variant ignores fields even with deny_unknown_fields.
fn daily_fields<'de, D: Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[expect(
        clippy::empty_structs_with_brackets,
        reason = "A named-field empty struct rejects extras; a unit struct would ignore them."
    )]
    struct Fields {}
    Fields::deserialize(deserializer).map(|_fields| ())
}

#[cfg(test)]
mod tests;
