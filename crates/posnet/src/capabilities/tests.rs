use super::capabilities;

#[test]
fn advertised_text_excludes_printer_table_glyphs_and_nonprinting_unicode() {
    let capabilities = capabilities();
    for text in [
        "\u{a4}", "\u{a8}", "\u{2db}", "\u{b4}", "\u{b8}", "\u{a0}", "\u{ad}", "\u{7f}", "Я", "😀",
        "\u{200b}", "a\u{301}", "\t",
    ] {
        assert!(!capabilities.allows(text), "{text:?}");
    }
    assert!(
        capabilities.allows("Zażółć gęślą jaźń – €"),
        "Polish names remain unchanged"
    );
}

#[test]
fn temo_limits_reject_overlong_receipt_fields_instead_of_truncating() {
    use crate::testing::{BRAND, line, pay};
    use fiscal::{
        error::Error,
        money::Grosze,
        receipt::{Line, PaymentForm, Quantity, Receipt, VatSlot},
    };

    let capabilities = capabilities();
    for (length, accepted) in [(80, true), (81, false)] {
        let name = "Ż".repeat(length);
        let result = Line::new(
            &name,
            Quantity::parse("1").unwrap(),
            Grosze::new(100).unwrap(),
            VatSlot::new(0).unwrap(),
            &capabilities,
        );
        if accepted {
            assert_eq!(result.unwrap().name(), name);
        } else {
            assert_eq!(
                result.unwrap_err().to_string(),
                "Nazwa usługi może mieć najwyżej 80 znaków."
            );
        }
    }
    for (count, accepted) in [(500, true), (501, false)] {
        let result = Receipt::new(
            vec![line("Olej", "1", "1", 0); count],
            vec![],
            vec![pay(PaymentForm::Cash, u64::try_from(count).unwrap() * 100)],
            None,
            BRAND,
            &capabilities,
        );
        if accepted {
            assert_eq!(result.unwrap().lines().len(), 500);
        } else {
            assert_eq!(
                result.unwrap_err().to_string(),
                "Paragon musi zawierać od 1 do 500 pozycji."
            );
        }
    }
    for (brand, nip, expected_error) in [
        ("Ż".repeat(40), "1".repeat(30), None),
        (
            "Ż".repeat(41),
            "1".repeat(30),
            Some(
                "Nazwa firmy musi mieć od 1 do 40 znaków obsługiwanych przez drukarkę. Zmień ją \
                 w profilu firmy w Holuto.",
            ),
        ),
        (
            "Ż".repeat(40),
            "1".repeat(31),
            Some("NIP nabywcy musi zawierać od 1 do 30 znaków."),
        ),
    ] {
        let result = Receipt::new(
            vec![line("Olej", "1", "1", 0)],
            vec![],
            vec![pay(PaymentForm::Cash, 100)],
            Some(nip.clone()),
            &brand,
            &capabilities,
        );
        if let Some(expected) = expected_error {
            assert!(
                matches!(&result, Err(Error::Invalid(message)) if message == expected),
                "{result:?}"
            );
        } else {
            let receipt = result.unwrap();
            assert_eq!(receipt.brand(), brand);
            assert_eq!(receipt.buyer_nip(), Some(nip.as_str()));
        }
    }
}
