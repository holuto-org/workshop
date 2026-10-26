//! Receipt fixtures shared by driver scenarios.

use fiscal::{
    money::Grosze,
    receipt::{Line, Payment, PaymentForm, Quantity, VatSlot},
};

use crate::{capabilities::capabilities, simulator::Params};

pub(crate) const BRAND: &str = "Warsztat Kowalski";

pub(crate) fn line(name: &str, quantity: &str, price: &str, slot: u8) -> Line {
    Line::new(
        name,
        Quantity::parse(quantity).unwrap(),
        Grosze::parse(price).unwrap(),
        VatSlot::new(slot).unwrap(),
        &capabilities(),
    )
    .unwrap()
}

pub(crate) fn pay(form: PaymentForm, amount: u64) -> Payment {
    Payment {
        form,
        amount: Grosze::new(amount).unwrap(),
    }
}

pub(crate) fn params(fields: &[(&str, &str)]) -> Params {
    fields
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}
