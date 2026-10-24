use proptest::prelude::*;

use super::{Advance, Line, Payment, PaymentForm, Quantity, Receipt, VatSlot};
use crate::{device::Capabilities, error::Error, money::Grosze};

pub(crate) const BRAND: &str = "Warsztat Kowalski";

/// Limits of a hypothetical driver; real values come from the driver crates.
pub(crate) fn capabilities() -> Capabilities {
    Capabilities {
        max_item_name: 80,
        max_lines: 500,
        max_company_name: 40,
        max_buyer_nip: 30,
        max_quantity_decimals: 8,
        allowed_characters: (' '..='~').chain("ĄĆĘŁŃÓŚŹŻąćęłńóśźż".chars()).collect(),
        advance_deductions: true,
        zero_total_receipts: true,
        company_footer: true,
    }
}

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

fn receipt(
    lines: Vec<Line>,
    advances: Vec<Advance>,
    payments: Vec<Payment>,
    nip: Option<&str>,
    brand: &str,
) -> Result<Receipt, Error> {
    Receipt::new(
        lines,
        advances,
        payments,
        nip.map(str::to_owned),
        brand,
        &capabilities(),
    )
}

#[test]
fn line_name_limit_follows_capabilities() {
    let capabilities = Capabilities {
        max_item_name: 64,
        ..capabilities()
    };
    let build = |name: String| {
        Line::new(
            &name,
            Quantity::parse("1").unwrap(),
            Grosze::new(1).unwrap(),
            VatSlot::new(0).unwrap(),
            &capabilities,
        )
    };
    assert_eq!(build("a".repeat(64)).unwrap().name(), "a".repeat(64));
    assert_eq!(
        build("a".repeat(65)).unwrap_err().to_string(),
        "Nazwa usługi może mieć najwyżej 64 znaków."
    );
}

#[test]
fn quantity_decimals_follow_capabilities() {
    let capabilities = Capabilities {
        max_quantity_decimals: 5,
        ..capabilities()
    };
    let build = |quantity: &str| {
        Line::new(
            "Olej",
            Quantity::parse(quantity).unwrap(),
            Grosze::new(100).unwrap(),
            VatSlot::new(0).unwrap(),
            &capabilities,
        )
    };
    assert_eq!(build("1.12345").unwrap().quantity().to_string(), "1.12345");
    assert_eq!(
        build("1.123456").unwrap_err().to_string(),
        "Ilość może mieć najwyżej 5 miejsc po przecinku."
    );
}

#[test]
fn receipt_limits_follow_capabilities() {
    let capabilities = Capabilities {
        max_lines: 2,
        max_company_name: 5,
        max_buyer_nip: 3,
        ..capabilities()
    };
    let sale = line("Olej", "1", "1", 0);
    let build = |lines: usize, nip: &str, brand: &str| {
        Receipt::new(
            vec![sale.clone(); lines],
            vec![],
            vec![pay(PaymentForm::Cash, 100 * u64::try_from(lines).unwrap())],
            Some(nip.to_owned()),
            brand,
            &capabilities,
        )
    };
    assert!(build(2, "123", "abcde").is_ok());
    for (result, message) in [
        (
            build(3, "123", "abcde"),
            "Paragon musi zawierać od 1 do 2 pozycji.",
        ),
        (
            build(2, "1234", "abcde"),
            "NIP nabywcy musi zawierać od 1 do 3 znaków.",
        ),
        (
            build(2, "123", "abcdef"),
            "Nazwa firmy musi mieć od 1 do 5 znaków obsługiwanych przez drukarkę. Zmień ją w \
             profilu firmy w Holuto.",
        ),
    ] {
        assert_eq!(result.unwrap_err().to_string(), message);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one named table keeps each receipt refusal beside its exact operator-facing reason"
)]
#[test]
fn receipt_validation_names_each_refusal_before_any_io() {
    let advance = Advance {
        vat: VatSlot::new(0).unwrap(),
        amount: Grosze::new(101).unwrap(),
    };
    let sale = line("Olej", "1", "1", 0);
    for (name, result, message) in [
        (
            "no lines",
            receipt(vec![], vec![], vec![], None, BRAND),
            "Paragon musi zawierać od 1 do 500 pozycji.",
        ),
        (
            "too many lines",
            receipt(vec![sale.clone(); 501], vec![], vec![], None, BRAND),
            "Paragon musi zawierać od 1 do 500 pozycji.",
        ),
        (
            "brand blank",
            receipt(vec![sale.clone()], vec![], vec![], None, " "),
            "Nazwa firmy musi mieć od 1 do 40 znaków obsługiwanych przez drukarkę. \
             Zmień ją w profilu firmy w Holuto.",
        ),
        (
            "brand too long",
            receipt(vec![sale.clone()], vec![], vec![], None, &"a".repeat(41)),
            "Nazwa firmy musi mieć od 1 do 40 znaków obsługiwanych przez drukarkę. \
             Zmień ją w profilu firmy w Holuto.",
        ),
        (
            "brand not printable",
            receipt(vec![sale.clone()], vec![], vec![], None, "😀"),
            "Nazwa firmy musi mieć od 1 do 40 znaków obsługiwanych przez drukarkę. \
             Zmień ją w profilu firmy w Holuto.",
        ),
        (
            "nip blank",
            receipt(vec![sale.clone()], vec![], vec![], Some(" "), BRAND),
            "NIP nabywcy musi zawierać od 1 do 30 znaków.",
        ),
        (
            "nip too long",
            receipt(
                vec![sale.clone()],
                vec![],
                vec![],
                Some(&"1".repeat(31)),
                BRAND,
            ),
            "NIP nabywcy musi zawierać od 1 do 30 znaków.",
        ),
        (
            "nip control",
            receipt(vec![sale.clone()], vec![], vec![], Some("1\t2"), BRAND),
            "NIP nabywcy zawiera niedozwolone znaki.",
        ),
        (
            "nip unicode",
            receipt(vec![sale.clone()], vec![], vec![], Some("😀"), BRAND),
            "NIP nabywcy zawiera niedozwolone znaki.",
        ),
        (
            "excess advance",
            receipt(vec![sale.clone()], vec![advance], vec![], None, BRAND),
            "Zaliczki przekraczają sprzedaż w slocie VAT.",
        ),
        (
            "missing slot",
            receipt(
                vec![sale.clone()],
                vec![Advance {
                    vat: VatSlot::new(1).unwrap(),
                    amount: Grosze::new(1).unwrap(),
                }],
                vec![],
                None,
                BRAND,
            ),
            "Zaliczki przekraczają sprzedaż w slocie VAT.",
        ),
        (
            "duplicate slot",
            receipt(
                vec![sale.clone()],
                vec![
                    Advance {
                        amount: Grosze::new(1).unwrap(),
                        ..advance
                    };
                    2
                ],
                vec![],
                None,
                BRAND,
            ),
            "Powtórzony slot VAT zaliczki. Wymagana jest jedna kwota na slot.",
        ),
        (
            "payment mismatch",
            receipt(vec![sale.clone()], vec![], vec![], None, BRAND),
            "Suma płatności nie odpowiada wartości fiskalnej paragonu.",
        ),
        (
            "zero advance",
            receipt(
                vec![sale],
                vec![Advance {
                    amount: Grosze::ZERO,
                    ..advance
                }],
                vec![pay(PaymentForm::Cash, 100)],
                None,
                BRAND,
            ),
            "Kwota zaliczki musi być dodatnia.",
        ),
        (
            "total overflow",
            receipt(
                vec![line("A", "1", "99999999.99", 0), line("B", "1", "0.01", 1)],
                vec![],
                vec![],
                None,
                BRAND,
            ),
            "Suma paragonu przekracza zakres drukarki.",
        ),
        (
            "slot overflow",
            receipt(
                vec![line("A", "1", "99999999.99", 0), line("B", "1", "0.01", 0)],
                vec![],
                vec![],
                None,
                BRAND,
            ),
            "Suma paragonu przekracza zakres drukarki.",
        ),
    ] {
        assert!(
            matches!(result, Err(Error::Invalid(_))),
            "{name}: {result:?}"
        );
        assert_eq!(result.unwrap_err().to_string(), message, "{name}");
    }
}

#[test]
fn line_validation_rejects_lossy_names_and_out_of_range_values() {
    for (name, quantity, price, message) in [
        (" ", "1", "1", "Nazwa usługi nie może być pusta."),
        (
            "😀",
            "1",
            "1",
            "Nazwa usługi zawiera znaki, których drukarka nie drukuje.",
        ),
        (
            "a\u{301}",
            "1",
            "1",
            "Nazwa usługi zawiera znaki, których drukarka nie drukuje.",
        ),
        (
            "Olej",
            "0",
            "1",
            "Ilość i cena muszą być dodatnimi liczbami.",
        ),
        (
            "Olej",
            "1",
            "0",
            "Ilość i cena muszą być dodatnimi liczbami.",
        ),
        (
            "Olej",
            "0.00000001",
            "0.01",
            "Wartość pozycji przekracza zakres drukarki lub zaokrągla się do zera.",
        ),
        (
            "Olej",
            "2",
            "99999999.99",
            "Wartość pozycji przekracza zakres drukarki lub zaokrągla się do zera.",
        ),
    ] {
        let result = Line::new(
            name,
            Quantity::parse(quantity).unwrap(),
            Grosze::parse(price).unwrap(),
            VatSlot::new(0).unwrap(),
            &capabilities(),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            message,
            "{name:?} {quantity} {price}"
        );
    }
    for name in ["Я", "a\tb", "\u{a0}", "\u{ad}", "\u{200b}", "¤"] {
        assert!(
            Line::new(
                name,
                Quantity::parse("1").unwrap(),
                Grosze::new(1).unwrap(),
                VatSlot::new(0).unwrap(),
                &capabilities(),
            )
            .is_err(),
            "{name:?}"
        );
    }
    assert_eq!(
        Line::new(
            &"a".repeat(81),
            Quantity::parse("1").unwrap(),
            Grosze::new(1).unwrap(),
            VatSlot::new(0).unwrap(),
            &capabilities(),
        )
        .unwrap_err()
        .to_string(),
        "Nazwa usługi może mieć najwyżej 80 znaków."
    );
    assert_eq!(
        VatSlot::new(7).unwrap_err().to_string(),
        "Nieprawidłowa stawka VAT."
    );
    for quantity in ["10000000000", "0.000000001", "1e2", "-1"] {
        assert!(Quantity::parse(quantity).is_err(), "{quantity}");
    }
}

#[test]
fn maximum_lengths_and_line_count_print_unchanged() {
    let name = "ą".repeat(80);
    let receipt = receipt(
        vec![line(&name, "1", "0.01", 0); 500],
        vec![],
        vec![pay(PaymentForm::Credit, 500)],
        Some(&"1".repeat(30)),
        &"ż".repeat(40),
    )
    .unwrap();
    assert_eq!(receipt.lines()[0].name(), name);
    assert_eq!(receipt.lines().len(), 500);
    assert_eq!(receipt.brand(), "ż".repeat(40));
    assert_eq!(receipt.fiscal_total().get(), 500);
    assert_eq!(
        Quantity::parse("9999999999").unwrap().to_string(),
        "9999999999"
    );
}

#[test]
fn half_up_rounding_is_correct_on_both_sides_of_the_tie() {
    for (quantity, expected) in [("0.49999999", 999), ("0.5", 1000), ("0.50000001", 1000)] {
        assert_eq!(
            line("Olej", quantity, "19.99", 0).total().get(),
            expected,
            "{quantity}"
        );
    }
    assert_eq!(Quantity::parse("9999999999").unwrap().times(u64::MAX), None);
}

#[test]
fn payment_forms_in_persisted_operations_have_stable_names() {
    for (form, name) in [
        (PaymentForm::Cash, "cash"),
        (PaymentForm::Card, "card"),
        (PaymentForm::Transfer, "transfer"),
        (PaymentForm::Mobile, "mobile"),
        (PaymentForm::Credit, "credit"),
    ] {
        let payment = pay(form, 123);
        let encoded = format!("{{\"form\":\"{name}\",\"amount\":123}}");
        assert_eq!(serde_json::to_string(&payment).unwrap(), encoded);
        assert_eq!(serde_json::from_str::<Payment>(&encoded).unwrap(), payment);
    }
}

proptest! {
    #[test]
    fn quantities_round_money_against_quotient_and_remainder_model(
        whole in 0..=9_999_999_998_u64,
        fraction in 0..100_000_000_u64,
        price in any::<u64>(),
    ) {
        let text = format!("{whole}.{fraction:08}");
        let exact = (u128::from(whole)*100_000_000 + u128::from(fraction))*u128::from(price);
        let quotient = exact / 100_000_000;
        let remainder = exact % 100_000_000;
        let expected = u64::try_from(quotient + u128::from(remainder >= 50_000_000)).ok();
        prop_assert_eq!(Quantity::parse(&text).unwrap().times(price),expected);
    }
}
