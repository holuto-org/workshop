use super::*;

#[test]
fn encodes_polish_text_as_windows_1250() {
    let bytes = [0xA3, 0xF3, 0x64, 0x9F, 0x20, 0xB9, 0xEA, 0x9C, 0xE6, 0xF1];
    assert_eq!(encode("Łódź ąęśćń"), Some(bytes.to_vec()));
    assert_eq!(decode(&bytes).as_deref(), Some("Łódź ąęśćń"));
    assert_eq!(printable_characters("Łódź"), Some(4));
}

#[test]
fn rejects_controls_unassigned_bytes_and_unrepresentable_text() {
    for text in ["a\rb", "a\nb", "a\tb", "\x1bP", "\x7f", "🙂"] {
        assert_eq!(encode(text), None, "{text:?}");
        assert_eq!(printable_characters(text), None, "{text:?}");
    }
    for byte in [0, 0x0D, 0x1B, 0x7F, 0x81, 0x83, 0x88, 0x90, 0x98] {
        assert_eq!(decode(&[byte]), None, "{byte:02X}");
    }
}
