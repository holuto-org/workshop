use super::capabilities;

#[test]
fn advertised_text_excludes_printer_table_glyphs_and_nonprinting_unicode() {
    let capabilities = capabilities();
    for text in [
        "\u{a4}", "\u{a8}", "\u{2db}", "\u{b4}", "\u{b8}", "\u{a0}", "\u{ad}", "\u{7f}", "Я", "😀",
        "\u{200b}", "a\u{301}", "\t",
    ] {
        assert!(!capabilities.allows(text), "{text:?}");
        assert!(
            !capabilities.allowed_characters.contains(text),
            "advertised {text:?}"
        );
    }
    assert!(
        capabilities.allows("Zażółć gęślą jaźń – €"),
        "Polish names remain unchanged"
    );
    assert_eq!(capabilities.allowed_characters.chars().count(), 211);
}

#[test]
fn limits_are_the_documented_posnet_ones() {
    let capabilities = capabilities();
    assert_eq!(capabilities.max_item_name, 80);
    assert_eq!(capabilities.max_lines, 500);
    assert_eq!(capabilities.max_company_name, 40);
    assert_eq!(capabilities.max_buyer_nip, 30);
    assert_eq!(capabilities.max_quantity_decimals, 8);
    assert!(capabilities.advance_deductions);
    assert!(capabilities.zero_total_receipts);
    assert!(capabilities.company_footer);
}
