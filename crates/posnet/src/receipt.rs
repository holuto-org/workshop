//! A fiscal receipt: lines on VAT slots, advance deductions and split payments, validated
//! before the printer is touched.

use std::{collections::BTreeMap, time::Duration};

use rust_decimal::{Decimal, prelude::ToPrimitive as _};
use serde::{Deserialize, Serialize};

use crate::{
    Error,
    money::{self, Grosze},
    text::{self, MAX_BRAND_LENGTH, MAX_BUYER_NIP_LENGTH, MAX_LINES, MAX_NAME_LENGTH},
};

/// The printer's VAT table has seven slots, A to G.
pub const VAT_SLOTS: u8 = 7;
const MAX_QUANTITY: Decimal = Decimal::from_parts(1_410_065_407, 2, 0, false, 0); // 9 999 999 999
const MAX_UNIT_PRICE: Decimal = Decimal::from_parts(1_410_065_407, 2, 0, false, 2); // 99 999 999.99
const QUANTITY_DECIMALS: u32 = 8;
const PRICE_DECIMALS: u32 = 2;
const LINE_TIMEOUT: Duration = Duration::from_secs(10);
const END_TIMEOUT: Duration = Duration::from_secs(30);
const ADVANCE_NAME: &str = "Zaliczka";
const MOBILE_NAME: &str = "Płatność mobilna";

/// A printer VAT slot, 0 (A) to 6 (G); not a rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct VatSlot(u8);

impl VatSlot {
    pub fn new(slot: u8) -> Result<Self, Error> {
        if slot >= VAT_SLOTS {
            return Err(Error::Invalid("Nieprawidłowa stawka VAT.".to_owned()));
        }
        Ok(Self(slot))
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for VatSlot {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Error> {
        Self::new(value)
    }
}

impl From<VatSlot> for u8 {
    fn from(value: VatSlot) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentForm {
    Cash,
    Card,
    Transfer,
    Mobile,
    Credit,
}

impl PaymentForm {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cash => "Gotówka",
            Self::Card => "Karta płatnicza",
            Self::Transfer => "Przelew",
            Self::Mobile => "Płatność mobilna",
            Self::Credit => "Kredyt",
        }
    }

    /// `trpayment` `ty` (spec p. 314).
    const fn wire_type(self) -> u8 {
        match self {
            Self::Cash => 0,
            Self::Card => 2,
            Self::Transfer => 8,
            Self::Mobile => 6,
            Self::Credit => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub form: PaymentForm,
    pub amount: Grosze,
}

/// An advance already invoiced on `vat`, deducted from that slot's sales.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Advance {
    pub vat: VatSlot,
    pub amount: Grosze,
}

/// One sale line. Prices are gross; the printer computes the VAT block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    name: String,
    quantity: Decimal,
    vat: VatSlot,
    price: Grosze,
    total: Grosze,
}

impl Line {
    /// `name` is sanitized for the printer (transliterated, controls removed, cut to 80
    /// characters); the line value is quantity × price rounded half up to grosze.
    pub fn new(
        name: &str,
        quantity: Decimal,
        unit_price: Decimal,
        vat: VatSlot,
    ) -> Result<Self, Error> {
        let name = text::sanitize(name, MAX_NAME_LENGTH);
        if name.is_empty() {
            return Err(Error::Invalid(
                "Nazwa usługi nie może być pusta.".to_owned(),
            ));
        }
        if quantity <= Decimal::ZERO || unit_price <= Decimal::ZERO {
            return Err(Error::Invalid(
                "Ilość i cena muszą być dodatnimi liczbami.".to_owned(),
            ));
        }
        let quantity = quantity.normalize();
        if quantity > MAX_QUANTITY || quantity.scale() > QUANTITY_DECIMALS {
            return Err(Error::Invalid(
                "Ilość może mieć najwyżej 8 miejsc po przecinku.".to_owned(),
            ));
        }
        let unit_price = unit_price.normalize();
        if unit_price > MAX_UNIT_PRICE || unit_price.scale() > PRICE_DECIMALS {
            return Err(Error::Invalid(
                "Cena może mieć najwyżej 2 miejsca po przecinku i wynosić do 99 999 999,99 zł."
                    .to_owned(),
            ));
        }
        let price = Grosze::from_zloty(unit_price)?;
        let total = quantity
            .checked_mul(unit_price)
            .and_then(|value| value.checked_mul(Decimal::ONE_HUNDRED))
            .map(money::half_up)
            .and_then(|grosze| grosze.to_u64())
            .and_then(|grosze| Grosze::new(grosze).ok())
            .filter(|total| *total > Grosze::ZERO)
            .ok_or_else(|| {
                Error::Invalid(
                    "Wartość pozycji przekracza zakres drukarki lub zaokrągla się do zera."
                        .to_owned(),
                )
            })?;
        Ok(Self {
            name,
            quantity,
            vat,
            price,
            total,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn quantity(&self) -> Decimal {
        self.quantity
    }

    pub const fn total(&self) -> Grosze {
        self.total
    }

    pub const fn price(&self) -> Grosze {
        self.price
    }
}

/// One command of an operation, with the time the printer may take to answer it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub command: &'static str,
    pub params: Vec<(&'static str, String)>,
    pub timeout: Duration,
}

impl Step {
    const fn new(
        command: &'static str,
        params: Vec<(&'static str, String)>,
        timeout: Duration,
    ) -> Self {
        Self {
            command,
            params,
            timeout,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    lines: Vec<Line>,
    advances: Vec<Advance>,
    payments: Vec<Payment>,
    buyer_nip: Option<String>,
    brand: String,
    sales: BTreeMap<VatSlot, Grosze>,
    fiscal_total: Grosze,
}

impl Receipt {
    /// Validates the settlement: at most one advance per slot, none above that slot's sales, and
    /// payments exactly covering the fiscal value (sales minus advances). `buyer_nip` is sent
    /// unchanged; `brand` (the company name) is printed as a footer when the header lacks it.
    pub fn new(
        lines: Vec<Line>,
        advances: Vec<Advance>,
        payments: Vec<Payment>,
        buyer_nip: Option<String>,
        brand: &str,
    ) -> Result<Self, Error> {
        if !(1..=MAX_LINES).contains(&lines.len()) {
            return Err(Error::Invalid(
                "Paragon musi zawierać od 1 do 500 pozycji.".to_owned(),
            ));
        }
        if let Some(nip) = &buyer_nip {
            if nip.trim().is_empty() || nip.chars().count() > MAX_BUYER_NIP_LENGTH {
                return Err(Error::Invalid(
                    "NIP nabywcy musi zawierać od 1 do 30 znaków.".to_owned(),
                ));
            }
            if text::check_printable(nip).is_err() {
                return Err(Error::Invalid(
                    "NIP nabywcy zawiera niedozwolone znaki.".to_owned(),
                ));
            }
        }
        let mut sales = BTreeMap::<VatSlot, Grosze>::new();
        for line in &lines {
            let slot = sales.entry(line.vat).or_default();
            *slot = slot
                .checked_add(line.total)
                .map_err(|_out_of_range| receipt_too_large())?;
        }
        let total = sales.values().copied().sum::<Grosze>();
        if total > Grosze::MAX {
            return Err(receipt_too_large());
        }
        let mut deducted = Grosze::ZERO;
        for (index, advance) in advances.iter().enumerate() {
            if advances
                .iter()
                .take(index)
                .any(|earlier| earlier.vat == advance.vat)
            {
                return Err(Error::Invalid(
                    "Powtórzony slot VAT zaliczki. Wymagana jest jedna kwota na slot.".to_owned(),
                ));
            }
            if sales
                .get(&advance.vat)
                .is_none_or(|slot_sales| advance.amount > *slot_sales)
            {
                return Err(Error::Invalid(
                    "Zaliczki przekraczają sprzedaż w slocie VAT.".to_owned(),
                ));
            }
            deducted = deducted.checked_add(advance.amount)?;
        }
        let fiscal_total = total.checked_sub(deducted).ok_or_else(receipt_too_large)?;
        if payments
            .iter()
            .map(|payment| payment.amount)
            .sum::<Grosze>()
            != fiscal_total
        {
            return Err(Error::Invalid(
                "Suma płatności nie odpowiada wartości fiskalnej paragonu.".to_owned(),
            ));
        }
        Ok(Self {
            lines,
            advances,
            payments,
            buyer_nip,
            brand: text::sanitize(brand, MAX_BRAND_LENGTH),
            sales,
            fiscal_total,
        })
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn payments(&self) -> &[Payment] {
        &self.payments
    }

    pub fn brand(&self) -> &str {
        &self.brand
    }

    /// Gross sales minus advances: what the payments cover.
    pub const fn fiscal_total(&self) -> Grosze {
        self.fiscal_total
    }

    pub(crate) fn slots(&self) -> impl Iterator<Item = VatSlot> + '_ {
        self.sales.keys().copied()
    }

    /// `trinit`, optional `trnipset`, a `trline` per line, a `tradvance` per deduction, a
    /// `trpayment` per nonzero payment, then `trend` (spec pp. 275-314). With a `footer` the
    /// receipt closes through the footer commands so the company name prints under it.
    pub fn steps(&self, footer: Option<&str>) -> Vec<Step> {
        let mut steps = vec![Step::new(
            "trinit",
            vec![("bm", "0".to_owned())],
            LINE_TIMEOUT,
        )];
        if let Some(nip) = &self.buyer_nip {
            steps.push(Step::new(
                "trnipset",
                vec![("ni", nip.clone())],
                LINE_TIMEOUT,
            ));
        }
        steps.extend(self.lines.iter().map(|line| {
            let params = vec![
                ("na", line.name.clone()),
                ("vt", line.vat.0.to_string()),
                ("pr", line.price().get().to_string()),
                ("il", line.quantity.to_string()),
                ("wa", line.total.get().to_string()),
            ];
            Step::new("trline", params, LINE_TIMEOUT)
        }));
        steps.extend(self.advances.iter().map(|advance| {
            let sales = self.sales.get(&advance.vat).copied().unwrap_or_default();
            let params = vec![
                ("na", ADVANCE_NAME.to_owned()),
                ("vt", advance.vat.0.to_string()),
                ("pr", advance.amount.get().to_string()),
                ("wa", sales.get().to_string()),
            ];
            Step::new("tradvance", params, LINE_TIMEOUT)
        }));
        steps.extend(
            self.payments
                .iter()
                .filter(|payment| payment.amount > Grosze::ZERO)
                .map(|payment| {
                    let mut params = vec![
                        ("ty", payment.form.wire_type().to_string()),
                        ("wa", payment.amount.get().to_string()),
                    ];
                    if payment.form == PaymentForm::Mobile {
                        params.push(("na", MOBILE_NAME.to_owned()));
                    }
                    Step::new("trpayment", params, LINE_TIMEOUT)
                }),
        );
        let paid = self
            .payments
            .iter()
            .map(|payment| payment.amount)
            .sum::<Grosze>();
        let mut end = vec![
            ("to", self.fiscal_total.get().to_string()),
            ("fp", paid.get().to_string()),
        ];
        if let Some(footer) = footer {
            end.push(("fe", "0".to_owned()));
            steps.push(Step::new("trend", end, END_TIMEOUT));
            steps.push(Step::new(
                "trftrln",
                vec![("id", "25".to_owned()), ("na", footer.to_owned())],
                LINE_TIMEOUT,
            ));
            steps.push(Step::new("trftrend", Vec::new(), END_TIMEOUT));
        } else {
            steps.push(Step::new("trend", end, END_TIMEOUT));
        }
        steps
    }
}

fn receipt_too_large() -> Error {
    Error::Invalid("Suma paragonu przekracza zakres drukarki.".to_owned())
}

#[cfg(test)]
pub(crate) mod tests;
