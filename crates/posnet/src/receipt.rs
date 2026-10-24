//! A fiscal receipt: lines on VAT slots, advance deductions and split payments, validated
//! before the printer is touched.

use std::{collections::BTreeMap, fmt, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    Error, decimal,
    money::Grosze,
    text::{self, MAX_BUYER_NIP_LENGTH, MAX_COMPANY_NAME_LENGTH, MAX_LINES, MAX_NAME_LENGTH},
};

/// The printer's VAT table has seven slots, A to G.
pub const VAT_SLOTS: u8 = 7;
const QUANTITY_SCALE: usize = 8;
const QUANTITY_UNIT: u64 = 100_000_000;
const MAX_QUANTITY: u64 = 9_999_999_999 * QUANTITY_UNIT;
const LINE_TIMEOUT: Duration = Duration::from_secs(10);
const END_TIMEOUT: Duration = Duration::from_secs(30);
const ADVANCE_NAME: &str = "Zaliczka";
const MOBILE_NAME: &str = "Płatność mobilna";

/// A printer VAT slot, 0 (A) to 6 (G); not a rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Advance {
    pub vat: VatSlot,
    pub amount: Grosze,
}

/// A line quantity in hundred-millionths: at most 8 decimal places, up to 9 999 999 999.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quantity(u64);

impl Quantity {
    /// A plain decimal such as `"1.5"`.
    pub fn parse(text: &str) -> Result<Self, Error> {
        decimal::parse(text, QUANTITY_SCALE)
            .filter(|units| *units <= MAX_QUANTITY)
            .map(Self)
            .ok_or_else(|| {
                Error::Invalid(
                    "Ilość musi być zapisana cyframi, z najwyżej 8 miejscami po przecinku, i \
                     wynosić najwyżej 9 999 999 999."
                        .to_owned(),
                )
            })
    }

    /// `self × grosze` rounded half up to whole grosze, as the printer rounds a line value;
    /// `None` above `u64`.
    pub(crate) fn times(self, grosze: u64) -> Option<u64> {
        // Below u128::MAX even for u64::MAX²: neither the product nor the half can overflow.
        let exact = u128::from(self.0) * u128::from(grosze);
        let unit = u128::from(QUANTITY_UNIT);
        u64::try_from((exact + unit / 2) / unit).ok()
    }
}

/// The `il` text the printer takes: `1.5`, `2`.
impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&decimal::format(self.0, QUANTITY_SCALE))
    }
}

/// One sale line. Prices are gross; the printer computes the VAT block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    name: String,
    quantity: Quantity,
    vat: VatSlot,
    price: Grosze,
    total: Grosze,
}

impl Line {
    /// `name` is printed exactly as given: at most 80 characters, all of them in
    /// [`text::printable_characters`], not blank. The line value is quantity × price rounded half
    /// up to grosze.
    pub fn new(
        name: &str,
        quantity: Quantity,
        unit_price: Grosze,
        vat: VatSlot,
    ) -> Result<Self, Error> {
        if name.trim().is_empty() {
            return Err(Error::Invalid(
                "Nazwa usługi nie może być pusta.".to_owned(),
            ));
        }
        if name.chars().count() > MAX_NAME_LENGTH {
            return Err(Error::Invalid(
                "Nazwa usługi może mieć najwyżej 80 znaków.".to_owned(),
            ));
        }
        if !text::is_printable_text(name) {
            return Err(Error::Invalid(
                "Nazwa usługi zawiera znaki, których drukarka nie drukuje.".to_owned(),
            ));
        }
        if quantity.0 == 0 || unit_price == Grosze::ZERO {
            return Err(Error::Invalid(
                "Ilość i cena muszą być dodatnimi liczbami.".to_owned(),
            ));
        }
        let total = quantity
            .times(unit_price.get())
            .and_then(|grosze| Grosze::new(grosze).ok())
            .filter(|total| *total > Grosze::ZERO)
            .ok_or_else(|| {
                Error::Invalid(
                    "Wartość pozycji przekracza zakres drukarki lub zaokrągla się do zera."
                        .to_owned(),
                )
            })?;
        Ok(Self {
            name: name.to_owned(),
            quantity,
            vat,
            price: unit_price,
            total,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
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
    /// unchanged. `brand`, the company name, is printed exactly as given as a footer when the
    /// header lacks it, so it must fit one footer line of printable characters.
    pub fn new(
        lines: Vec<Line>,
        advances: Vec<Advance>,
        payments: Vec<Payment>,
        buyer_nip: Option<String>,
        brand: &str,
    ) -> Result<Self, Error> {
        if brand.trim().is_empty()
            || brand.chars().count() > MAX_COMPANY_NAME_LENGTH
            || !text::is_printable_text(brand)
        {
            return Err(Error::Invalid(
                "Nazwa firmy musi mieć od 1 do 40 znaków obsługiwanych przez drukarkę. Zmień ją \
                 w profilu firmy w Holuto."
                    .to_owned(),
            ));
        }
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
        }
        // At most one advance per slot, none above its slot's sales: this stays within `total`.
        let deducted = advances
            .iter()
            .map(|advance| advance.amount)
            .sum::<Grosze>();
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
            brand: brand.to_owned(),
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
        // `new` only accepts payments that sum exactly to the fiscal total.
        let mut end = vec![
            ("to", self.fiscal_total.get().to_string()),
            ("fp", self.fiscal_total.get().to_string()),
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
