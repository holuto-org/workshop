use super::{
    check_printable, decode, encode, header_text, is_printable_text, printable_characters,
};

#[test]
fn polish_text_round_trips_through_windows_1250() {
    let text = "Zażółć gęślą jaźń ŁÓDŹ €";
    let bytes = encode(text).unwrap();
    assert_eq!(bytes.get(..5), Some(b"Za\xBF\xF3\xB3".as_slice()));
    assert_eq!(decode(&bytes).as_deref(), Some(text));
}

#[test]
fn unassigned_code_page_bytes_are_not_text() {
    for byte in [0x81, 0x83, 0x88, 0x90, 0x98] {
        assert_eq!(decode(&[b'a', byte]), None, "byte {byte:#04X}");
    }
}

#[test]
fn advertised_characters_are_exactly_what_receipt_text_accepts() {
    let characters = printable_characters();
    let unassigned = [0x81, 0x83, 0x88, 0x90, 0x98];
    let unprintable = [0x7F, 0xA0, 0xAD]; // DEL, no-break space, soft hyphen
    let expected: Vec<u8> = (0x20..=0xFF_u8)
        .filter(|byte| !unassigned.contains(byte) && !unprintable.contains(byte))
        .collect();
    assert_eq!(characters.chars().count(), 216);
    assert_eq!(encode(&characters), Some(expected));
    for character in [' ', '&', '€', 'Ą', 'ż', 'ó', 'ß'] {
        assert!(characters.contains(character), "{character:?}");
    }
    assert!(!characters.contains('\t') && !characters.contains('Ж'));
    check_printable(&characters).unwrap();
    assert!(is_printable_text(&characters));
    for character in [
        '\t', '\u{A0}', '\u{AD}', '\u{7F}', 'Ж', '🔧', '\u{200B}', '\u{301}',
    ] {
        assert!(!is_printable_text(&character.to_string()), "{character:?}");
    }
}

#[test]
fn frame_text_rejects_controls_and_characters_without_a_byte() {
    for text in ["x\ty", "x\u{3}", "x\n", "🔧"] {
        assert!(check_printable(text).is_err(), "{text:?}");
    }
}

#[test]
fn header_formatting_marks_are_stripped_and_a_doubled_ampersand_is_literal() {
    assert_eq!(
        header_text("&c&1Kowalski && Syn&1&c\nul. X"),
        "Kowalski & Syn\nul. X"
    );
}
