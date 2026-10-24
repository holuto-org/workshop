//! Holuto documents eligible for a receipt, and the operator's request to queue one.

use jiff::Timestamp;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Error, api::nullable};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReceiptKind {
    /// An advance payment: identified by its payment allocation, one line per VAT rate.
    Advance,
    /// The final sale of a posted order; the unpaid rest prints as credit.
    Sale,
}

impl ReceiptKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Advance => "Zaliczka",
            Self::Sale => "Sprzedaż",
        }
    }

    /// An advance names its allocation; a sale has none.
    pub(crate) fn check_allocation(self, allocation_id: Option<Uuid>) -> Result<(), String> {
        match (self, allocation_id) {
            (Self::Advance, Some(_)) | (Self::Sale, None) => Ok(()),
            (Self::Advance, None) => Err("zaliczka bez identyfikatora alokacji".to_owned()),
            (Self::Sale, Some(_)) => Err("sprzedaż z identyfikatorem alokacji".to_owned()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub kind: ReceiptKind,
    pub order_id: Uuid,
    pub order_number: i64,
    #[serde(deserialize_with = "nullable")]
    pub allocation_id: Option<Uuid>,
    /// Listed even above the printer's limit; execution enforces it.
    pub amount: Decimal,
    #[serde(deserialize_with = "nullable")]
    pub customer_name: Option<String>,
    pub occurred_at: Timestamp,
}

impl Document {
    /// Whether both describe the same printable document (an order can have several advances).
    pub fn same_document(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.order_id == other.order_id
            && self.allocation_id == other.allocation_id
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        self.kind.check_allocation(self.allocation_id)?;
        if self.order_number <= 0 {
            return Err("numer zlecenia musi być dodatni".to_owned());
        }
        if self.amount.is_sign_negative() {
            return Err("ujemna kwota dokumentu".to_owned());
        }
        Ok(())
    }
}

/// A buyer's tax number printed on the receipt: exactly ten digits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BuyerNip(String);

impl BuyerNip {
    pub fn parse(text: &str) -> Result<Self, Error> {
        let text = text.trim();
        if text.len() == 10 && text.bytes().all(|byte| byte.is_ascii_digit()) {
            Ok(Self(text.to_owned()))
        } else {
            Err(Error::Invalid(
                "NIP nabywcy musi zawierać 10 cyfr.".to_owned(),
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests;
