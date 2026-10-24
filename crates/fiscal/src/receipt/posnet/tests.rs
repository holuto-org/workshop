use super::{is_printable_text, printable_characters};

#[test]
fn advertised_text_excludes_printer_table_glyphs_and_nonprinting_unicode() {
    for text in [
        "\u{a4}", "\u{a8}", "\u{2db}", "\u{b4}", "\u{b8}", "\u{a0}", "\u{ad}", "\u{7f}", "Я", "😀",
        "\u{200b}", "a\u{301}", "\t",
    ] {
        assert!(!is_printable_text(text), "{text:?}");
        assert!(
            !printable_characters().contains(text),
            "advertised {text:?}"
        );
    }
    assert!(
        is_printable_text("Zażółć gęślą jaźń – €"),
        "Polish names remain unchanged"
    );
    assert_eq!(printable_characters().chars().count(), 211);
}
