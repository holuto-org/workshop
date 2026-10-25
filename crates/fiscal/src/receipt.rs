//! Fiscal receipts validated against the device's capabilities before printer I/O.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

use crate::{decimal, device::Capabilities, error::Error, money::Grosze};

/// VAT slots A–G.
pub const VAT_SLOTS: u8 = 7;
const QUANTITY_SCALE: usize = 8;
const QUANTITY_UNIT: u64 = 100_000_000;
const MAX_QUANTITY: u64 = 9_999_999_999 * QUANTITY_UNIT;

/// VAT slot 0 (A) through 6 (G), not a rate.
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub form: PaymentForm,
    pub amount: Grosze,
}

/// Already-invoiced advance deducted from sales in the same VAT slot.
#[derive(Clone, Copy, Debug)]
pub struct Advance {
    pub vat: VatSlot,
    pub amount: Grosze,
}

/// Quantity in hundred-millionths, up to 9 999 999 999 with at most eight decimal places.
#[derive(Clone, Copy, Debug)]
pub struct Quantity(u64);

impl Quantity {
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

    pub const fn units(self) -> u64 {
        self.0
    }

    /// Whether no digit lies beyond `decimals` places after the decimal point.
    fn fits_decimals(self, decimals: u8) -> bool {
        10_u64
            .checked_pow(decimals.into())
            .filter(|places| *places <= QUANTITY_UNIT)
            .is_none_or(|places| self.0.is_multiple_of(QUANTITY_UNIT / places))
    }

    /// Matches printer line rounding: half up to whole grosze; `None` on overflow.
    pub fn times(self, grosze: u64) -> Option<u64> {
        // u64::MAX² plus half a quantity unit fits u128.
        let exact = u128::from(self.0) * u128::from(grosze);
        let unit = u128::from(QUANTITY_UNIT);
        u64::try_from((exact + unit / 2) / unit).ok()
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&decimal::format(self.0, QUANTITY_SCALE))
    }
}

/// Gross prices; the printer computes VAT.
#[derive(Clone, Debug)]
pub struct Line {
    name: String,
    quantity: Quantity,
    vat: VatSlot,
    price: Grosze,
    total: Grosze,
}

impl Line {
    /// Prints `name` unchanged: nonblank, within the item-name limit and entirely in the
    /// device's allowed characters. The quantity must fit its decimal places.
    /// Line value is quantity × price, rounded half up to grosze.
    pub fn new(
        name: &str,
        quantity: Quantity,
        unit_price: Grosze,
        vat: VatSlot,
        capabilities: &Capabilities,
    ) -> Result<Self, Error> {
        if name.trim().is_empty() {
            return Err(Error::Invalid(
                "Nazwa usługi nie może być pusta.".to_owned(),
            ));
        }
        if name.chars().count() > capabilities.max_item_name {
            return Err(Error::Invalid(format!(
                "Nazwa usługi może mieć najwyżej {} znaków.",
                capabilities.max_item_name
            )));
        }
        if !capabilities.allows(name) {
            return Err(Error::Invalid(
                "Nazwa usługi zawiera znaki, których drukarka nie drukuje.".to_owned(),
            ));
        }
        if !quantity.fits_decimals(capabilities.max_quantity_decimals) {
            return Err(Error::Invalid(format!(
                "Ilość może mieć najwyżej {} miejsc po przecinku.",
                capabilities.max_quantity_decimals
            )));
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

    pub const fn quantity(&self) -> Quantity {
        self.quantity
    }

    pub const fn vat(&self) -> VatSlot {
        self.vat
    }
}

#[derive(Debug)]
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
    /// Requires at most one positive advance per slot, bounded by that slot's sales.
    /// Payments must equal sales minus advances. `buyer_nip` is sent unchanged.
    /// `brand` prints unchanged when absent from the header and must fit one footer line.
    /// Line count, brand, NIP and their characters follow `capabilities`.
    pub fn new(
        lines: Vec<Line>,
        advances: Vec<Advance>,
        payments: Vec<Payment>,
        buyer_nip: Option<String>,
        brand: &str,
        capabilities: &Capabilities,
    ) -> Result<Self, Error> {
        if brand.trim().is_empty()
            || brand.chars().count() > capabilities.max_company_name
            || !capabilities.allows(brand)
        {
            return Err(Error::Invalid(format!(
                "Nazwa firmy musi mieć od 1 do {} znaków obsługiwanych przez drukarkę. Zmień ją \
                 w profilu firmy w Holuto.",
                capabilities.max_company_name
            )));
        }
        if !(1..=capabilities.max_lines).contains(&lines.len()) {
            return Err(Error::Invalid(format!(
                "Paragon musi zawierać od 1 do {} pozycji.",
                capabilities.max_lines
            )));
        }
        if let Some(nip) = &buyer_nip {
            if nip.trim().is_empty() || nip.chars().count() > capabilities.max_buyer_nip {
                return Err(Error::Invalid(format!(
                    "NIP nabywcy musi zawierać od 1 do {} znaków.",
                    capabilities.max_buyer_nip
                )));
            }
            if !capabilities.allows(nip) {
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
            if advance.amount == Grosze::ZERO {
                return Err(Error::Invalid(
                    "Kwota zaliczki musi być dodatnia.".to_owned(),
                ));
            }
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
        // Per-slot bounds above ensure deductions cannot exceed total sales.
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

    pub fn advances(&self) -> &[Advance] {
        &self.advances
    }

    pub fn buyer_nip(&self) -> Option<&str> {
        self.buyer_nip.as_deref()
    }

    pub fn payments(&self) -> &[Payment] {
        &self.payments
    }

    pub fn brand(&self) -> &str {
        &self.brand
    }

    /// Case-insensitive; whitespace runs match, since printers pad or break header lines.
    pub fn brand_in_header(&self, header: &str) -> bool {
        let words = |text: &str| {
            text.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        words(header).contains(&words(&self.brand))
    }

    /// Gross sales minus advances, covered by payments.
    pub const fn fiscal_total(&self) -> Grosze {
        self.fiscal_total
    }

    pub const fn sales(&self) -> &BTreeMap<VatSlot, Grosze> {
        &self.sales
    }
}

fn receipt_too_large() -> Error {
    Error::Invalid("Suma paragonu przekracza zakres drukarki.".to_owned())
}

#[cfg(test)]
mod tests;
