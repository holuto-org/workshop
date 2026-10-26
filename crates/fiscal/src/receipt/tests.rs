use std::collections::BTreeMap;

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
    assert_eq!(build("ą".repeat(64)).unwrap().name(), "ą".repeat(64));
    assert_eq!(
        build("ą".repeat(65)).unwrap_err().to_string(),
        "Nazwa usługi może mieć najwyżej 64 znaków."
    );
}

#[test]
fn quantity_decimals_follow_capabilities_without_counting_trailing_zeros() {
    for (decimals, accepted, units, rejected) in [
        (0, "1.00000000", 100_000_000, "1.00000001"),
        (5, "1.12345000", 112_345_000, "1.123456"),
        (7, "1.00000010", 100_000_010, "1.00000001"),
    ] {
        let capabilities = Capabilities {
            max_quantity_decimals: decimals,
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
        assert_eq!(build(accepted).unwrap().quantity().units(), units);
        assert_eq!(
            build(rejected).unwrap_err().to_string(),
            format!("Ilość może mieć najwyżej {decimals} miejsc po przecinku.")
        );
    }
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
    let accepted = build(2, "ą12", "żółta").unwrap();
    assert_eq!(
        accepted.lines().iter().map(Line::name).collect::<Vec<_>>(),
        ["Olej", "Olej"]
    );
    assert_eq!(accepted.buyer_nip(), Some("ą12"));
    assert_eq!(accepted.brand(), "żółta");
    assert_eq!(accepted.fiscal_total().get(), 200);
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
            "brand blank",
            receipt(vec![sale.clone()], vec![], vec![], None, " "),
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
    for (name, message) in [
        (
            "Я",
            "Nazwa usługi zawiera znaki, których drukarka nie drukuje.",
        ),
        (
            "a\tb",
            "Nazwa usługi zawiera znaki, których drukarka nie drukuje.",
        ),
        ("\u{a0}", "Nazwa usługi nie może być pusta."),
        (
            "\u{ad}",
            "Nazwa usługi zawiera znaki, których drukarka nie drukuje.",
        ),
        (
            "\u{200b}",
            "Nazwa usługi zawiera znaki, których drukarka nie drukuje.",
        ),
        (
            "¤",
            "Nazwa usługi zawiera znaki, których drukarka nie drukuje.",
        ),
    ] {
        let error = Line::new(
            name,
            Quantity::parse("1").unwrap(),
            Grosze::new(1).unwrap(),
            VatSlot::new(0).unwrap(),
            &capabilities(),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Invalid(_)), "{name:?}");
        assert_eq!(error.to_string(), message, "{name:?}");
    }
}

#[test]
fn maximum_lengths_and_line_count_are_retained_unchanged() {
    let name = "ą".repeat(80);
    let receipt = receipt(
        vec![line(&name, "1", "0.01", 0); 500],
        vec![],
        vec![pay(PaymentForm::Credit, 500)],
        Some(&"1".repeat(30)),
        &"ż".repeat(40),
    )
    .unwrap();
    assert_eq!(
        receipt.lines().iter().map(Line::name).collect::<Vec<_>>(),
        vec![name.as_str(); 500]
    );
    assert_eq!(receipt.buyer_nip(), Some("111111111111111111111111111111"));
    assert_eq!(receipt.brand(), "ż".repeat(40));
    assert_eq!(receipt.fiscal_total().get(), 500);
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

#[test]
fn quantity_and_vat_slot_boundaries_are_exact() {
    let maximum = Quantity::parse("9999999999.00000000").unwrap();
    assert_eq!(maximum.units(), 999_999_999_900_000_000);
    assert_eq!(maximum.to_string(), "9999999999");
    assert_eq!(Quantity::parse("0.00000001").unwrap().units(), 1);
    for text in [
        "9999999999.00000001",
        "10000000000",
        "0.000000001",
        "1e2",
        "-1",
    ] {
        let error = Quantity::parse(text).unwrap_err();
        assert!(matches!(error, Error::Invalid(_)));
        assert_eq!(
            error.to_string(),
            "Ilość musi być zapisana cyframi, z najwyżej 8 miejscami po przecinku, i \
             wynosić najwyżej 9 999 999 999.",
            "{text}"
        );
    }
    for slot in 0..=6 {
        assert_eq!(VatSlot::new(slot).unwrap().get(), slot);
    }
    for slot in [7, u8::MAX] {
        assert_eq!(
            VatSlot::new(slot).unwrap_err().to_string(),
            "Nieprawidłowa stawka VAT."
        );
    }
}

#[test]
fn split_payments_must_cover_the_post_advance_total_exactly_even_at_zero() {
    let build = |payments| {
        receipt(
            vec![line("Olej", "1", "1", 0)],
            vec![Advance {
                vat: VatSlot::new(0).unwrap(),
                amount: Grosze::new(40).unwrap(),
            }],
            payments,
            None,
            BRAND,
        )
    };
    let paid = build(vec![pay(PaymentForm::Cash, 20), pay(PaymentForm::Card, 40)]).unwrap();
    assert_eq!(paid.fiscal_total().get(), 60);
    assert_eq!(
        paid.payments(),
        [pay(PaymentForm::Cash, 20), pay(PaymentForm::Card, 40)]
    );
    for payments in [
        vec![pay(PaymentForm::Cash, 59)],
        vec![pay(PaymentForm::Cash, 61)],
        vec![pay(PaymentForm::Cash, 60), pay(PaymentForm::Credit, 1)],
        vec![
            pay(PaymentForm::Cash, 9_999_999_999),
            pay(PaymentForm::Card, 1),
        ],
    ] {
        assert_eq!(
            build(payments).unwrap_err().to_string(),
            "Suma płatności nie odpowiada wartości fiskalnej paragonu."
        );
    }
    let prepaid = receipt(
        vec![line("Olej", "1", "99999999.99", 6)],
        vec![Advance {
            vat: VatSlot::new(6).unwrap(),
            amount: Grosze::MAX,
        }],
        vec![],
        Some(" PL-123 "),
        BRAND,
    )
    .unwrap();
    assert_eq!(
        prepaid
            .sales()
            .get(&VatSlot::new(6).unwrap())
            .unwrap()
            .get(),
        9_999_999_999
    );
    assert_eq!(prepaid.fiscal_total().get(), 0);
    assert_eq!(prepaid.buyer_nip(), Some(" PL-123 "));
    assert_eq!(prepaid.payments(), []);
}

proptest! {
    #[test]
    fn quantities_round_money_against_quotient_and_remainder_model(
        whole in prop_oneof![0..=100_u64, 0..=9_999_999_998_u64],
        fraction in prop_oneof![
            Just(0), Just(49_999_999), Just(50_000_000), Just(50_000_001),
            0..100_000_000_u64,
        ],
        price in prop_oneof![0..=10_000_u64, Just(9_999_999_999), any::<u64>()],
    ) {
        let text = format!("{whole}.{fraction:08}");
        let exact = (u128::from(whole)*100_000_000 + u128::from(fraction))*u128::from(price);
        let quotient = exact / 100_000_000;
        let remainder = exact % 100_000_000;
        let expected = u64::try_from(quotient + u128::from(remainder >= 50_000_000)).ok();
        prop_assert_eq!(Quantity::parse(&text).unwrap().times(price),expected);
    }

    #[test]
    fn line_rounding_and_range_follow_an_independent_integer_oracle(
        units in prop_oneof![
            1..=10_000_000_000_u64, Just(1), Just(49_999_999), Just(50_000_000),
            Just(50_000_001), Just(999_999_999_900_000_000),
        ],
        price in prop_oneof![1..=9_999_999_999_u64, Just(1), Just(9_999_999_999)],
    ) {
        let quantity = format!("{}.{:08}", units / 100_000_000, units % 100_000_000);
        let product = u128::from(units) * u128::from(price);
        let expected = product / 100_000_000 + u128::from(product % 100_000_000 >= 50_000_000);
        let result = Line::new(
            "Łódź", Quantity::parse(&quantity).unwrap(), Grosze::new(price).unwrap(),
            VatSlot::new(6).unwrap(), &capabilities(),
        );
        if (1..=9_999_999_999).contains(&expected) {
            let line = result.unwrap();
            prop_assert_eq!(u128::from(line.total().get()), expected);
            prop_assert_eq!(line.name(), "Łódź");
            prop_assert_eq!(line.quantity().units(), units);
            prop_assert_eq!(line.price().get(), price);
            prop_assert_eq!(line.vat().get(), 6);
        } else {
            prop_assert_eq!(
                result.unwrap_err().to_string(),
                "Wartość pozycji przekracza zakres drukarki lub zaokrągla się do zera."
            );
        }
    }

    #[test]
    fn per_slot_sales_deductions_and_split_payments_are_order_independent(
        first in prop::array::uniform7(1..=1_000_000_u64),
        second in prop::array::uniform7(1..=1_000_000_u64),
        deductions in prop::array::uniform7(any::<u64>()),
        split in any::<u64>(),
    ) {
        let mut lines = Vec::new();
        let mut advances = Vec::new();
        let mut expected_sales = BTreeMap::new();
        let mut expected_total = 0;
        for slot in 0..7_u8 {
            let index = usize::from(slot);
            let sales = first[index] + second[index];
            let deducted = deductions[index] % (sales + 1);
            let vat = VatSlot::new(slot).unwrap();
            for price in [first[index], second[index]] {
                lines.push(Line::new(
                    "Olej", Quantity::parse("1").unwrap(), Grosze::new(price).unwrap(),
                    vat, &capabilities(),
                ).unwrap());
            }
            if deducted > 0 {
                advances.push(Advance { vat, amount: Grosze::new(deducted).unwrap() });
            }
            let _previous = expected_sales.insert(vat, Grosze::new(sales).unwrap());
            expected_total += sales - deducted;
        }
        let cash = split % (expected_total + 1);
        let mut payments = vec![
            pay(PaymentForm::Cash, cash),
            pay(PaymentForm::Transfer, expected_total - cash),
        ];
        let original = receipt(
            lines.clone(), advances.clone(), payments.clone(), Some(" PL-123 "), BRAND,
        ).unwrap();
        prop_assert_eq!(original.sales(), &expected_sales);
        prop_assert_eq!(original.fiscal_total().get(), expected_total);
        prop_assert_eq!(original.payments(), &payments);
        prop_assert_eq!(
            original.advances().iter()
                .map(|advance| (advance.vat, advance.amount)).collect::<Vec<_>>(),
            advances.iter().map(|advance| (advance.vat, advance.amount)).collect::<Vec<_>>()
        );
        lines.reverse();
        advances.reverse();
        payments.reverse();
        let reversed = receipt(lines, advances, payments, Some(" PL-123 "), BRAND).unwrap();
        prop_assert_eq!(reversed.sales(), &expected_sales);
        prop_assert_eq!(reversed.fiscal_total().get(), expected_total);
        prop_assert_eq!(reversed.buyer_nip(), Some(" PL-123 "));
    }
}
