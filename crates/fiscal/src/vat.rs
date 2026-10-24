//! VAT slots with the existing Posnet rate representation.

use crate::decimal;

/// The printer encodes VAT exemption as 100 %.
const EXEMPT_BASIS_POINTS: u32 = 10_000;
const MAX_ACTIVE_BASIS_POINTS: u32 = 9_999;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VatRate {
    pub slot: u8,
    /// Hundredths of a percent: `2300` is 23 %.
    pub basis_points: u32,
}

impl VatRate {
    /// Above 99.99 %, only the exempt 100 % slot is active.
    pub const fn active(self) -> bool {
        self.exempt() || self.basis_points <= MAX_ACTIVE_BASIS_POINTS
    }

    pub const fn exempt(self) -> bool {
        self.basis_points == EXEMPT_BASIS_POINTS
    }

    pub fn label(self) -> String {
        let letter = char::from(b'A'.saturating_add(self.slot));
        let rate = if self.exempt() {
            "zwolniona".to_owned()
        } else if self.active() {
            format!("{}%", decimal::format(self.basis_points.into(), 2)).replace('.', ",")
        } else {
            "nieaktywna".to_owned()
        };
        format!("{letter} · {rate}")
    }
}
