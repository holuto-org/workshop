use std::fmt::Debug;

use rust_decimal::Decimal;

use super::{Advance, Line, Payment, PaymentForm, Receipt, VatSlot};
use crate::{Error, money::Grosze};

fn decimal(text: &str) -> Decimal {
    text.parse().unwrap()
}

fn slot(slot: u8) -> VatSlot {
    VatSlot::new(slot).unwrap()
}

fn grosze(amount: u64) -> Grosze {
    Grosze::new(amount).unwrap()
}

pub(crate) fn line(name: &str, quantity: &str, price: &str, vat: u8) -> Line {
    Line::new(name, decimal(quantity), decimal(price), slot(vat)).unwrap()
}

pub(crate) fn pay(form: PaymentForm, amount: u64) -> Payment {
    Payment {
        form,
        amount: grosze(amount),
    }
}

fn advance(vat: u8, amount: u64) -> Advance {
    Advance {
        vat: slot(vat),
        amount: grosze(amount),
    }
}

fn invalid(result: Result<impl Debug, Error>) -> String {
    match result {
        Err(Error::Invalid(message)) => message,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn fractional_quantity_rounds_the_line_half_up() {
    let line = line("Naprawa", "0.5", "19.99", 0);
    assert_eq!(line.total(), grosze(1000));
    assert_eq!(line.price(), grosze(1999));
}

#[test]
fn quantity_price_and_slot_must_fit_the_printer() {
    for quantity in ["0", "-1", "0.000000001", "10000000000"] {
        assert!(
            Line::new("Naprawa", decimal(quantity), Decimal::TEN, slot(0)).is_err(),
            "{quantity}"
        );
    }
    for price in ["0", "-1", "1.001", "100000000"] {
        let message = invalid(Line::new("Naprawa", Decimal::ONE, decimal(price), slot(0)));
        assert!(
            message.contains("Cena") || message.contains("dodatnimi"),
            "{price}: {message}"
        );
    }
    assert!(Line::new("Naprawa", decimal("9999999999"), Decimal::TEN, slot(0)).is_err());
    assert!(Line::new("Naprawa", decimal("0.001"), decimal("0.01"), slot(0)).is_err());
    assert!(VatSlot::new(7).is_err());
}

#[test]
fn a_name_that_sanitizes_to_nothing_is_rejected() {
    for name in ["", " \t", "\u{AD}"] {
        assert_eq!(
            invalid(Line::new(name, Decimal::ONE, Decimal::TEN, slot(0))),
            "Nazwa usługi nie może być pusta."
        );
    }
}

#[test]
fn quantity_prints_without_trailing_zeros() {
    assert_eq!(
        line("Olej", "2.500", "1.00", 0).quantity().to_string(),
        "2.5"
    );
}

#[test]
fn receipt_arguments_are_checked() {
    let one = line("Olej", "1", "1.00", 0);
    let biggest = line("Olej", "1", "99999999.99", 0);
    let cases: Vec<(Vec<Line>, Vec<Payment>, &str)> = vec![
        (vec![], vec![], "od 1 do 500"),
        (vec![one.clone(); 501], vec![], "od 1 do 500"),
        (vec![biggest.clone(), biggest], vec![], "przekracza zakres"),
        (
            vec![one],
            vec![pay(PaymentForm::Cash, 99)],
            "Suma płatności",
        ),
    ];
    for (lines, payments, problem) in cases {
        let message = invalid(Receipt::new(lines, vec![], payments, None, ""));
        assert!(message.contains(problem), "{message}");
    }
}

#[test]
fn invalid_buyer_nip_is_rejected() {
    for nip in ["", "   ", &"1".repeat(31), "123\tni456", "123\n456", "🔧"] {
        let result = Receipt::new(
            vec![line("Olej", "1", "1.00", 0)],
            vec![],
            vec![pay(PaymentForm::Cash, 100)],
            Some(nip.to_owned()),
            "",
        );
        assert!(invalid(result).contains("NIP"), "{nip:?}");
    }
}

#[test]
fn settlement_must_balance_per_slot_and_overall() {
    let lines = || vec![line("Usługa", "1", "1.00", 0)];
    for (advances, payments) in [
        (vec![advance(0, 80), advance(0, 30)], vec![]),
        (
            vec![advance(0, 40), advance(0, 10)],
            vec![pay(PaymentForm::Cash, 50)],
        ),
        (vec![advance(1, 10)], vec![pay(PaymentForm::Cash, 90)]),
        (vec![advance(0, 101)], vec![]),
        (vec![], vec![pay(PaymentForm::Cash, 99)]),
        (vec![], vec![pay(PaymentForm::Cash, 101)]),
    ] {
        assert!(
            Receipt::new(lines(), advances.clone(), payments, None, "").is_err(),
            "{advances:?}"
        );
    }
    let message = invalid(Receipt::new(
        lines(),
        vec![advance(0, 40), advance(0, 10)],
        vec![],
        None,
        "",
    ));
    assert!(
        message.contains("Powtórzony slot VAT zaliczki"),
        "{message}"
    );
}

#[test]
fn fully_prepaid_receipt_closes_at_zero_without_payment_commands() {
    let lines = vec![line("Usługa", "1", "10.00", 0)];
    for brand in ["", "Inna firma"] {
        let receipt =
            Receipt::new(lines.clone(), vec![advance(0, 1000)], vec![], None, brand).unwrap();
        let footer = (!brand.is_empty()).then_some(brand);
        let steps = receipt.steps(footer);
        assert!(steps.iter().all(|step| step.command != "trpayment"));
        let end = steps.iter().find(|step| step.command == "trend").unwrap();
        let mut expected = vec![("to", "0".to_owned()), ("fp", "0".to_owned())];
        if footer.is_some() {
            expected.push(("fe", "0".to_owned()));
        }
        assert_eq!(end.params, expected);
    }
}

#[test]
fn zero_payments_send_no_command_and_mobile_payments_carry_their_name() {
    let receipt = Receipt::new(
        vec![line("Usługa", "1", "10.00", 0)],
        vec![],
        vec![pay(PaymentForm::Cash, 0), pay(PaymentForm::Mobile, 1000)],
        Some("5222628262".to_owned()),
        "",
    )
    .unwrap();
    let steps = receipt.steps(None);
    let commands: Vec<&str> = steps.iter().map(|step| step.command).collect();
    assert_eq!(
        commands,
        ["trinit", "trnipset", "trline", "trpayment", "trend"]
    );
    assert_eq!(
        steps[3].params,
        [
            ("ty", "6".to_owned()),
            ("wa", "1000".to_owned()),
            ("na", "Płatność mobilna".to_owned())
        ]
    );
}
