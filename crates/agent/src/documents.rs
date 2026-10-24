use fiscal::decimal;
use jiff::Timestamp;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error as _, Unexpected},
};
use std::fmt;
use uuid::Uuid;

use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReceiptKind {
    /// Payment allocation with one line per VAT rate.
    Advance,
    /// Posted order's final sale; unpaid balance prints as credit.
    Sale,
}

impl ReceiptKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Advance => "Zaliczka",
            Self::Sale => "Sprzedaż",
        }
    }

    pub(crate) fn check_allocation(self, allocation_id: Option<Uuid>) -> Result<(), String> {
        match (self, allocation_id) {
            (Self::Advance, Some(_)) | (Self::Sale, None) => Ok(()),
            (Self::Advance, None) => Err("zaliczka bez identyfikatora alokacji".to_owned()),
            (Self::Sale, Some(_)) => Err("sprzedaż z identyfikatorem alokacji".to_owned()),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub kind: ReceiptKind,
    pub order_id: Uuid,
    pub order_number: i64,
    #[serde(deserialize_with = "Option::deserialize")]
    pub allocation_id: Option<Uuid>,
    /// Grosze; execution, not listing, enforces the printer's limit.
    #[serde(deserialize_with = "grosze", serialize_with = "zloty")]
    pub amount: u64,
    #[serde(deserialize_with = "Option::deserialize")]
    pub customer_name: Option<String>,
    pub occurred_at: Timestamp,
}

impl fmt::Debug for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Document")
            .field("kind", &self.kind)
            .field("order_id", &self.order_id)
            .finish_non_exhaustive()
    }
}

impl Document {
    /// An order may have multiple advances, each identified by its allocation.
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
        Ok(())
    }
}

/// Converts a złoty string with at most two decimal places to grosze.
fn grosze<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let zloty = String::deserialize(deserializer)?;
    decimal::parse(&zloty, 2).ok_or_else(|| {
        D::Error::invalid_value(
            Unexpected::Str(&zloty),
            &"a złoty amount with at most two decimal places",
        )
    })
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's field serializer callback takes a reference"
)]
fn zloty<S: Serializer>(amount: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{}.{:02}", amount / 100, amount % 100))
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BuyerNip(String);

impl fmt::Debug for BuyerNip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BuyerNip([REDACTED])")
    }
}

impl BuyerNip {
    pub(crate) fn into_string(self) -> String {
        self.0
    }

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
}

#[cfg(test)]
mod tests;
