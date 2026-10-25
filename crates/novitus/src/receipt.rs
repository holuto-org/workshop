//! Build the complete receipt before any fiscal send.
//! Revalidate against this printer: a Receipt may target another device.

use std::time::Duration;

use fiscal::{
    decimal,
    error::Error,
    money::Grosze,
    receipt::{PaymentForm, Receipt},
};

use crate::{profile::Profile, text};

#[derive(Debug)]
pub(crate) struct Step {
    pub command: &'static str,
    pub body: String,
    pub timeout: Duration,
}
impl Step {
    pub(crate) const fn new(command: &'static str, body: String) -> Self {
        Self {
            command,
            body,
            timeout: Duration::from_secs(3),
        }
    }
}

fn validate(receipt: &Receipt, footer: Option<&str>, profile: &Profile) -> Result<(), Error> {
    let caps = &profile.capabilities;
    if (!receipt.advances().is_empty() && !caps.advance_deductions)
        || (receipt.fiscal_total() == Grosze::ZERO && !caps.zero_total_receipts)
        || (footer.is_some() && !caps.company_footer)
    {
        return Err(Error::Invalid(
            "Drukarka nie obsługuje zaliczek, zerowej kwoty lub wymaganej stopki firmy.".to_owned(),
        ));
    }
    if receipt.lines().len() > caps.max_lines
        || receipt.brand().chars().count() > caps.max_company_name
        || !caps.allows(receipt.brand())
        || receipt.buyer_nip().is_some_and(|nip| {
            nip.len() > caps.max_buyer_nip || !nip.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        return Err(Error::Invalid(
            "Paragon przekracza limity tekstu lub liczby pozycji Novitus.".to_owned(),
        ));
    }
    for line in receipt.lines() {
        if line.name().chars().count() > caps.max_item_name || !caps.allows(line.name()) {
            return Err(Error::Invalid(
                "Nazwa pozycji przekracza możliwości Novitus.".to_owned(),
            ));
        }
        let quantity = line.quantity().to_string();
        // Novitus's quantity range is narrower than Quantity's.
        if quantity.bytes().filter(u8::is_ascii_digit).count() > 10 || quantity.len() > 16 {
            return Err(Error::Invalid(
                "Ilość przekracza zakres protokołu Novitus.".to_owned(),
            ));
        }
    }
    if receipt
        .payments()
        .iter()
        .filter(|payment| payment.amount != Grosze::ZERO)
        .count()
        > 16
    {
        return Err(Error::Invalid(
            "Novitus obsługuje najwyżej 16 form płatności.".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn steps(
    receipt: &Receipt,
    footer: Option<&str>,
    profile: &Profile,
) -> Result<Vec<Step>, Error> {
    validate(receipt, footer, profile)?;
    let mut steps = vec![Step::new("$h", "0$h".to_owned())];
    if let Some(nip) = receipt.buyer_nip() {
        steps.push(Step::new("$z", format!("100;1$z{nip}\r")));
    }
    let mut remaining = receipt
        .advances()
        .iter()
        .map(|advance| (advance.vat, advance.amount.get()))
        .collect::<Vec<_>>();
    for (index, line) in receipt.lines().iter().enumerate() {
        let letter = char::from(b'A' + line.vat().get());
        steps.push(Step::new(
            "$l",
            format!(
                "{}$l{}\r{}\r{letter}/{}/{}/",
                index + 1,
                line.name(),
                line.quantity(),
                decimal::format(line.price().get(), 2),
                decimal::format(line.total().get(), 2)
            ),
        ));
        // First-match advance allocation is unverified; acceptance only.
        if let Some((_, remaining)) = remaining.iter_mut().find(|(vat, _)| *vat == line.vat()) {
            let amount = (*remaining).min(line.total().get());
            if amount > 0 {
                steps.push(Step::new(
                    "@z",
                    format!(
                        "1;@z{}\r{letter}\r{}/{}/",
                        line.name(),
                        decimal::format(amount, 2),
                        decimal::format(line.total().get() - amount, 2)
                    ),
                ));
                *remaining -= amount;
            }
        }
    }
    for payment in receipt
        .payments()
        .iter()
        .filter(|payment| payment.amount != Grosze::ZERO)
    {
        let kind = match payment.form {
            PaymentForm::Cash => 0,
            PaymentForm::Card => 1,
            PaymentForm::Transfer => 8,
            PaymentForm::Mobile => 9,
            PaymentForm::Credit => 5,
        };
        steps.push(Step::new(
            "$b",
            format!("1;{kind}$b{}/\r", decimal::format(payment.amount.get(), 2)),
        ));
    }
    let total = decimal::format(receipt.fiscal_total().get(), 2);
    steps.push(Step::new(
        "$y",
        format!("0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r{total}/0/0/{total}/0/"),
    ));
    steps.push(Step::new("$z", "12$z".to_owned()));
    if let Some(footer) = footer {
        if text::printable_characters(footer).is_none_or(|length| length > profile.info_line_width)
        {
            return Err(Error::Invalid(
                "Nazwa firmy nie mieści się w stopce Novitus.".to_owned(),
            ));
        }
        steps.push(Step::new("$w", format!("200;0$w{footer}\r")));
    }
    steps.push(Step::new("$z", "28;0$z".to_owned()));
    Ok(steps)
}
