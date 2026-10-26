use super::{decode, encode};

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
fn text_encoding_and_decoding_preserve_order_and_refuse_the_whole_invalid_field() {
    let text = "AĄąŁłÓóŹźŻż 09";
    let bytes = b"A\xA5\xB9\xA3\xB3\xD3\xF3\x8F\x9F\xAF\xBF 09";
    assert_eq!(encode(text), Some(bytes.to_vec()));
    assert_eq!(decode(bytes).as_deref(), Some(text));
    assert_eq!(encode(""), Some(vec![]));
    assert_eq!(decode(b""), Some(String::new()));
    for invalid in ['😀', 'Я', '\u{301}', '\u{fffd}'] {
        assert_eq!(encode(&format!("Łódź{invalid}ABC")), None, "{invalid}");
    }
    for invalid in [0x81, 0x83, 0x88, 0x90, 0x98] {
        assert_eq!(decode(&[b'A', invalid, b'B']), None, "0x{invalid:02X}");
    }
}
