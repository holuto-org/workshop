use super::*;
#[test]
fn printable_cp1250_round_trips_every_advertised_character() {
    let repertoire = printable_cp1250();
    let bytes = encode(&repertoire).unwrap();
    assert_eq!(cp1250::decode(&bytes).unwrap(), repertoire);
    assert!(!repertoire.chars().any(char::is_control));
    assert_eq!(encode("Łódź €").unwrap(), b"\xA3\xF3d\x9F \x80");
}
#[test]
fn text_never_injects_control_separators_or_substitutes_glyphs() {
    for text in [
        "LOR\x1dC0",
        "\x02",
        "\x03",
        "a\n",
        "a\r",
        "a\t",
        "\x7f",
        "😀",
    ] {
        assert!(matches!(encode(text), Err(Error::Invalid(_))));
    }
    assert_eq!(
        encode_body("LNP\x1dOAŁódź\x1eBx\x1fA1").unwrap(),
        b"LNP\x1dOA\xA3\xF3d\x9F\x1eBx\x1fA1"
    );
    assert!(matches!(encode_body("LOR\x02"), Err(Error::Invalid(_))));
}
