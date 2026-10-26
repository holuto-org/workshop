use super::*;
#[test]
fn printable_cp1250_round_trips_every_advertised_character() {
    let repertoire = printable_cp1250();
    let bytes = cp1250::encode(&repertoire).unwrap();
    let expected: Vec<_> = (0x20..=u8::MAX)
        .filter(|byte| !matches!(byte, 0x7F | 0x81 | 0x83 | 0x88 | 0x90 | 0x98))
        .collect();
    assert_eq!(bytes, expected);
    assert_eq!(cp1250::decode(&bytes).unwrap(), repertoire);
    assert!(!repertoire.chars().any(char::is_control));
}
