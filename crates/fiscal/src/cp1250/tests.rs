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
