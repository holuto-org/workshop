//! Validate the entire command sequence before LOR; no substitution or rounding.
use crate::{frame, text};
use fiscal::{
    device::Capabilities,
    error::Error,
    money::Grosze,
    receipt::{PaymentForm, Receipt},
};
use std::time::Duration;

/// Item, NIP, advance and payment commands answer once the line is accepted.
const LINE_TIMEOUT: Duration = Duration::from_secs(10);
/// Closing commands may finish the printout before answering.
const END_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub(crate) struct Step {
    pub command: &'static str,
    pub fields: String,
    /// Silence beyond this leaves the outcome uncertain; nothing is resent.
    pub timeout: Duration,
}
impl Step {
    pub(crate) const fn new(command: &'static str, fields: String, timeout: Duration) -> Self {
        Self {
            command,
            fields,
            timeout,
        }
    }
}
pub(crate) fn amount(amount: Grosze) -> String {
    format!("{},{:02}", amount.get() / 100, amount.get() % 100)
}
fn validate(receipt: &Receipt, footer: Option<&str>, caps: &Capabilities) -> Result<(), Error> {
    if !receipt.advances().is_empty() && !caps.advance_deductions {
        return Err(Error::Invalid(
            "Drukarka nie obsługuje rozliczenia zaliczek.".to_owned(),
        ));
    }
    if receipt.fiscal_total() == Grosze::ZERO && !caps.zero_total_receipts {
        return Err(Error::Invalid(
            "Drukarka nie obsługuje paragonu opłaconego w całości zaliczkami.".to_owned(),
        ));
    }
    // The name stays out of the message: failure reasons are stored and sent onward.
    if footer.is_some() && !caps.company_footer {
        return Err(Error::Invalid(
            "Nagłówek drukarki nie zawiera nazwy firmy z Holuto, a ta drukarka nie drukuje jej \
             w stopce. Wpisz w Holuto nazwę zgodną z nagłówkiem drukarki."
                .to_owned(),
        ));
    }
    if receipt.lines().len() > caps.max_lines
        || receipt.brand().chars().count() > caps.max_company_name
        || !caps.allows(receipt.brand())
        || receipt
            .buyer_nip()
            .is_some_and(|nip| nip.chars().count() > caps.max_buyer_nip || !caps.allows(nip))
    {
        return Err(Error::Invalid(
            "Dane paragonu przekraczają możliwości Elzab.".to_owned(),
        ));
    }
    for line in receipt.lines() {
        if line.name().chars().count() > caps.max_item_name
            || !caps.allows(line.name())
            || !line.quantity().units().is_multiple_of(1000)
        {
            return Err(Error::Invalid(
                "Pozycja nie mieści się w profilu Elzab (64 znaki, 5 miejsc ilości).".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn steps(
    receipt: &Receipt,
    footer: Option<&str>,
    caps: &Capabilities,
) -> Result<Vec<Step>, Error> {
    validate(receipt, footer, caps)?;
    // ONLINE line-by-line open variant, never sent to legacy ZETA.
    let mut result = vec![Step::new("LOR", "C0".to_owned(), LINE_TIMEOUT)];
    if let Some(nip) = receipt.buyer_nip() {
        result.push(Step::new("LNP", format!("OA{nip}"), LINE_TIMEOUT));
    }
    for line in receipt.lines() {
        let letter = char::from(b'A' + line.vat().get());
        result.push(Step::new(
            "LIT",
            format!(
                "GA{}\x1eB{letter}\x1eC{}\x1eD{}\x1eE{}",
                line.name(),
                line.quantity().to_string().replace('.', ","),
                amount(line.price()),
                amount(line.total())
            ),
            LINE_TIMEOUT,
        ));
    }
    for advance in receipt.advances() {
        let letter = char::from(b'A' + advance.vat.get());
        result.push(Step::new(
            "LDS",
            format!(
                "IAA1\x1fB{}\x1fDZaliczka\x1eC{letter}\x1eD",
                amount(advance.amount)
            ),
            LINE_TIMEOUT,
        ));
    }
    for (index, payment) in receipt
        .payments()
        .iter()
        .filter(|p| p.amount != Grosze::ZERO)
        .enumerate()
    {
        if index >= 7 {
            return Err(Error::Invalid(
                "Elzab przyjmuje najwyżej 7 wpisów płatności.".to_owned(),
            ));
        }
        let (kind, name) = match payment.form {
            PaymentForm::Cash => (1, "GOTÓWKA"),
            PaymentForm::Card => (2, "KARTA"),
            PaymentForm::Transfer => (7, "PRZELEW"),
            PaymentForm::Mobile => (9, "MOBILNA"),
            PaymentForm::Credit => (6, "KREDYT"),
        };
        result.push(Step::new(
            "LBP",
            format!(
                "KA{}\x1eB{name}\x1eC{}\x1eX{kind}",
                index + 1,
                amount(payment.amount)
            ),
            LINE_TIMEOUT,
        ));
    }
    result.push(Step::new(
        "LCL",
        format!("RA{}", amount(receipt.fiscal_total())),
        END_TIMEOUT,
    ));
    // Acceptance can exercise this order; production needs hardware qualification
    // of post-close LAL before enabling company_footer.
    if let Some(footer) = footer {
        result.push(Step::new("LAL", format!("QAA{footer}"), END_TIMEOUT));
    }
    for step in &result {
        drop(frame::encode(
            "999999999",
            &text::encode_body(step.command, &step.fields)?,
        )?);
    }
    Ok(result)
}
