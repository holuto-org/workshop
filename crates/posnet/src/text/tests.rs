use super::{decode, encode, header_text, is_printable_text, printable_characters};

#[test]
fn every_code_page_byte_matches_the_independent_unicode_reference() {
    // Unmodified mapping: https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/WINDOWS/CP1250.TXT
    let mut reference = [None; 256];
    for line in include_str!("CP1250.TXT")
        .lines()
        .filter(|line| line.starts_with("0x"))
    {
        let fields: Vec<_> = line.split('#').next().unwrap().split_whitespace().collect();
        let byte = u8::from_str_radix(fields[0].trim_start_matches("0x"), 16).unwrap();
        reference[usize::from(byte)] = fields.get(1).map(|code| {
            char::from_u32(u32::from_str_radix(code.trim_start_matches("0x"), 16).unwrap()).unwrap()
        });
    }
    for byte in 0..=255_u8 {
        let expected = reference[usize::from(byte)].map(|character| character.to_string());
        assert_eq!(decode(&[byte]), expected, "decode 0x{byte:02X}");
        if let Some(expected) = expected {
            assert_eq!(encode(&expected), Some(vec![byte]), "encode 0x{byte:02X}");
        }
    }
}

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

#[test]
fn header_matching_uses_only_the_text_the_printer_displays() {
    assert_eq!(
        header_text("&c&1Auto && Moto&1&c\nŻółw&"),
        "Auto & Moto\nŻółw"
    );
}
