use std::fmt::Debug;

use super::{Advance, Line, Payment, PaymentForm, Quantity, Receipt, VatSlot};
use crate::{Error, money::Grosze};

fn quantity(text: &str) -> Quantity {
    Quantity::parse(text).unwrap()
}

fn price(zloty: &str) -> Grosze {
    Grosze::parse(zloty).unwrap()
}

fn slot(slot: u8) -> VatSlot {
    VatSlot::new(slot).unwrap()
}

fn grosze(amount: u64) -> Grosze {
    Grosze::new(amount).unwrap()
}

/// The simulator's header names this company, so receipts carrying it print no footer.
pub(crate) const BRAND: &str = "Warsztat Kowalski";

pub(crate) fn line(name: &str, quantity_text: &str, zloty: &str, vat: u8) -> Line {
    Line::new(name, quantity(quantity_text), price(zloty), slot(vat)).unwrap()
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
    for text in ["-1", "0.000000001", "10000000000", "1e3", "NaN"] {
        assert!(Quantity::parse(text).is_err(), "{text}");
    }
    assert_eq!(quantity("1.50000000").to_string(), "1.5");
    assert_eq!(quantity("9999999999").to_string(), "9999999999");
    let ten = price("10");
    for (count, unit) in [(quantity("0"), ten), (quantity("1"), Grosze::ZERO)] {
        let message = invalid(Line::new("Naprawa", count, unit, slot(0)));
        assert!(message.contains("dodatnimi"), "{message}");
    }
    for (count, unit) in [
        (quantity("9999999999"), ten),
        (quantity("0.001"), price("0.01")),
    ] {
        let message = invalid(Line::new("Naprawa", count, unit, slot(0)));
        assert!(message.contains("Wartość pozycji"), "{message}");
    }
    assert!(VatSlot::new(7).is_err());
}

#[test]
fn names_print_exactly_as_given_or_are_rejected() {
    for name in ["Zażółć gęślą jaźń €", " Olej ", &"ł".repeat(80)] {
        assert_eq!(line(name, "1", "1.00", 0).name(), name);
    }
    let cases = [
        ("", "pusta"),
        ("   ", "pusta"),
        (&"ł".repeat(81), "najwyżej 80"),
        ("Кузов", "nie drukuje"),
        ("Wymiana 🔧 oleju", "nie drukuje"),
        ("x\tnaInjected", "nie drukuje"),
        ("Olej\u{AD}", "nie drukuje"),
        ("Olej\u{A0}5W30", "nie drukuje"),
        ("a\u{200B}b", "nie drukuje"),
        ("o\u{301}", "nie drukuje"),
    ];
    for (name, problem) in cases {
        let message = invalid(Line::new(name, quantity("1"), price("10"), slot(0)));
        assert!(message.contains(problem), "{name:?}: {message}");
    }
}

#[test]
fn the_company_name_must_fit_one_printable_footer_line() {
    let receipt = |brand: &str| {
        Receipt::new(
            vec![line("Olej", "1", "1.00", 0)],
            vec![],
            vec![pay(PaymentForm::Cash, 100)],
            None,
            brand,
        )
    };
    for brand in ["Łódź „Serwis” – Kowalski", &"ł".repeat(40)] {
        assert_eq!(receipt(brand).unwrap().brand(), brand);
    }
    for brand in [
        "",
        "  ",
        &"ł".repeat(41),
        "Кузов",
        "Firma\u{A0}X",
        "Firma\tX",
    ] {
        assert!(invalid(receipt(brand)).contains("Nazwa firmy"), "{brand:?}");
    }
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
        let message = invalid(Receipt::new(lines, vec![], payments, None, BRAND));
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
            BRAND,
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
            Receipt::new(lines(), advances.clone(), payments, None, BRAND).is_err(),
            "{advances:?}"
        );
    }
    let message = invalid(Receipt::new(
        lines(),
        vec![advance(0, 40), advance(0, 10)],
        vec![],
        None,
        BRAND,
    ));
    assert!(
        message.contains("Powtórzony slot VAT zaliczki"),
        "{message}"
    );
}

#[test]
fn fully_prepaid_receipt_closes_at_zero_without_payment_commands() {
    let lines = vec![line("Usługa", "1", "10.00", 0)];
    let receipt = Receipt::new(lines, vec![advance(0, 1000)], vec![], None, BRAND).unwrap();
    for footer in [None, Some("Inna firma")] {
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
        BRAND,
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
