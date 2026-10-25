//! P:3583-3620,3804-3888,4131-4166,4287-4312,4578-4589,4828-4841,4903-4918.
//! Validate the entire command sequence before LOR; no substitution or rounding.
use crate::{frame, profile::Profile, text};
use fiscal::{
    error::Error,
    money::Grosze,
    receipt::{PaymentForm, Receipt},
};

#[derive(Debug)]
pub(crate) struct Step {
    pub command: &'static str,
    pub body: String,
}
impl Step {
    pub(crate) const fn new(command: &'static str, body: String) -> Self {
        Self { command, body }
    }
}
pub(crate) fn amount(amount: Grosze) -> String {
    format!("{},{:02}", amount.get() / 100, amount.get() % 100)
}
fn validate(receipt: &Receipt, footer: Option<&str>, profile: &Profile) -> Result<(), Error> {
    let caps = &profile.capabilities;
    if (!receipt.advances().is_empty() && !caps.advance_deductions)
        || (receipt.fiscal_total() == Grosze::ZERO && !caps.zero_total_receipts)
        || (footer.is_some() && !caps.company_footer)
    {
        return Err(Error::Invalid(
            "Profil Elzab nie obsługuje tego rozliczenia lub tekstu firmy.".to_owned(),
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
        drop(text::encode(line.name())?);
    }
    drop(text::encode(receipt.brand())?);
    if let Some(nip) = receipt.buyer_nip() {
        drop(text::encode(nip)?);
    }
    if let Some(footer) = footer {
        drop(text::encode(footer)?);
    }
    Ok(())
}

pub(crate) fn steps(
    receipt: &Receipt,
    footer: Option<&str>,
    profile: &Profile,
) -> Result<Vec<Step>, Error> {
    validate(receipt, footer, profile)?;
    let mut result = vec![Step::new("LOR", profile.open.to_owned())];
    if let Some(nip) = receipt.buyer_nip() {
        result.push(Step::new("LNP", format!("LNP\x1dOA{nip}")));
    }
    for line in receipt.lines() {
        let letter = char::from(b'A' + line.vat().get());
        result.push(Step::new(
            "LIT",
            format!(
                "LIT\x1dGA{}\x1eB{letter}\x1eC{}\x1eD{}\x1eE{}",
                line.name(),
                line.quantity().to_string().replace('.', ","),
                amount(line.price()),
                amount(line.total())
            ),
        ));
    }
    for advance in receipt.advances() {
        let letter = char::from(b'A' + advance.vat.get());
        result.push(Step::new(
            "LDS",
            format!(
                "LDS\x1dIAA1\x1fB{}\x1fDZaliczka\x1eC{letter}\x1eD",
                amount(advance.amount)
            ),
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
                "LBP\x1dKA{}\x1eB{name}\x1eC{}\x1eX{kind}",
                index + 1,
                amount(payment.amount)
            ),
        ));
    }
    result.push(Step::new(
        "LCL",
        format!("LCL\x1dRA{}", amount(receipt.fiscal_total())),
    ));
    // Never reached by candidate profiles. A qualified profile must establish
    // this post-close LAL ordering before enabling company_footer (OSS §2).
    if let Some(footer) = footer {
        result.push(Step::new("LAL", format!("LAL\x1dQAA{footer}")));
    }
    for step in &result {
        drop(frame::encode("999999999", &text::encode_body(&step.body)?)?);
    }
    Ok(result)
}
