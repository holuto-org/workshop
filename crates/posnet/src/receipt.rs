//! Receipt command order and payment codes (POT-I-DEV-37 v5978, pp. 275–314).

use std::time::Duration;

use fiscal::{
    money::Grosze,
    receipt::{PaymentForm, Receipt},
};

const LINE_TIMEOUT: Duration = Duration::from_secs(10);
const END_TIMEOUT: Duration = Duration::from_secs(30);
const ADVANCE_NAME: &str = "Zaliczka";
const MOBILE_NAME: &str = "Płatność mobilna";

#[derive(Debug)]
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

const fn wire_type(form: PaymentForm) -> u8 {
    match form {
        PaymentForm::Cash => 0,
        PaymentForm::Card => 2,
        PaymentForm::Transfer => 8,
        PaymentForm::Mobile => 6,
        PaymentForm::Credit => 5,
    }
}

pub fn steps(receipt: &Receipt, footer: Option<&str>) -> Vec<Step> {
    let mut steps = vec![Step::new(
        "trinit",
        vec![("bm", "0".to_owned())],
        LINE_TIMEOUT,
    )];
    if let Some(nip) = receipt.buyer_nip() {
        steps.push(Step::new(
            "trnipset",
            vec![("ni", nip.to_owned())],
            LINE_TIMEOUT,
        ));
    }
    steps.extend(receipt.lines().iter().map(|line| {
        let params = vec![
            ("na", line.name().to_owned()),
            ("vt", line.vat().get().to_string()),
            ("pr", line.price().get().to_string()),
            ("il", line.quantity().to_string()),
            ("wa", line.total().get().to_string()),
        ];
        Step::new("trline", params, LINE_TIMEOUT)
    }));
    steps.extend(receipt.advances().iter().map(|advance| {
        let sales = receipt
            .sales()
            .get(&advance.vat)
            .copied()
            .unwrap_or_default();
        let params = vec![
            ("na", ADVANCE_NAME.to_owned()),
            ("vt", advance.vat.get().to_string()),
            ("pr", advance.amount.get().to_string()),
            ("wa", sales.get().to_string()),
        ];
        Step::new("tradvance", params, LINE_TIMEOUT)
    }));
    steps.extend(
        receipt
            .payments()
            .iter()
            .filter(|payment| payment.amount > Grosze::ZERO)
            .map(|payment| {
                let mut params = vec![
                    ("ty", wire_type(payment.form).to_string()),
                    ("wa", payment.amount.get().to_string()),
                ];
                if payment.form == PaymentForm::Mobile {
                    params.push(("na", MOBILE_NAME.to_owned()));
                }
                Step::new("trpayment", params, LINE_TIMEOUT)
            }),
    );
    // Validated payments equal the fiscal total.
    let mut end = vec![
        ("to", receipt.fiscal_total().get().to_string()),
        ("fp", receipt.fiscal_total().get().to_string()),
    ];
    if footer.is_some() {
        end.push(("fe", "0".to_owned()));
    }
    steps.push(Step::new("trend", end, END_TIMEOUT));
    if let Some(footer) = footer {
        steps.push(Step::new(
            "trftrln",
            vec![("id", "25".to_owned()), ("na", footer.to_owned())],
            LINE_TIMEOUT,
        ));
        steps.push(Step::new("trftrend", Vec::new(), END_TIMEOUT));
    }
    steps
}
