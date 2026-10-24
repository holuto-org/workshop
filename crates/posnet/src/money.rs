//! Amounts in grosze, the unit every fiscal command carries.

use std::{fmt, iter::Sum};

use rust_decimal::{Decimal, RoundingStrategy, prelude::ToPrimitive as _};
use serde::{Deserialize, Serialize};

use crate::Error;

/// A nonnegative amount within the printer's range: 9 999 999 999 grosze.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(try_from = "u64", into = "u64")]
pub struct Grosze(u64);

impl Grosze {
    pub const MAX: Self = Self(9_999_999_999);
    pub const ZERO: Self = Self(0);

    pub fn new(grosze: u64) -> Result<Self, Error> {
        if grosze > Self::MAX.0 {
            return Err(out_of_range());
        }
        Ok(Self(grosze))
    }

    /// A decimal amount in złoty that is a whole number of grosze, e.g. `"12.50"`.
    pub fn from_zloty(zloty: Decimal) -> Result<Self, Error> {
        let grosze = zloty
            .checked_mul(Decimal::ONE_HUNDRED)
            .ok_or_else(out_of_range)?;
        if !grosze.fract().is_zero() || grosze.is_sign_negative() {
            return Err(out_of_range());
        }
        Self::new(grosze.to_u64().ok_or_else(out_of_range)?)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, Error> {
        Self::new(self.0.checked_add(other.0).ok_or_else(out_of_range)?)
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }
}

impl TryFrom<u64> for Grosze {
    type Error = Error;

    fn try_from(value: u64) -> Result<Self, Error> {
        Self::new(value)
    }
}

impl From<Grosze> for u64 {
    fn from(value: Grosze) -> Self {
        value.0
    }
}

/// Saturates at one past [`Grosze::MAX`] so an oversized sum fails the range check, never wraps.
impl Sum for Grosze {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Self(
            iter.fold(0_u64, |total, item| total.saturating_add(item.0))
                .min(Self::MAX.0 + 1),
        )
    }
}

/// `1 234 567,89 zł`, the way a Polish receipt writes it.
impl fmt::Display for Grosze {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = (self.0 / 100).to_string();
        let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
        for (index, digit) in whole.chars().enumerate() {
            if index > 0 && (whole.len() - index).is_multiple_of(3) {
                grouped.push(' ');
            }
            grouped.push(digit);
        }
        write!(f, "{grouped},{:02} zł", self.0 % 100)
    }
}

/// Rounds to whole units, halves away from zero: how the printer rounds a line value.
pub fn half_up(value: Decimal) -> Decimal {
    value.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
}

fn out_of_range() -> Error {
    Error::Invalid("Kwota musi być nieujemna i mieścić się w zakresie drukarki.".to_owned())
}

#[cfg(test)]
mod tests;
