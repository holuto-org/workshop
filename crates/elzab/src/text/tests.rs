use super::*;
#[test]
fn printable_cp1250_round_trips_every_advertised_character() {
    let repertoire = printable_cp1250();
    let bytes = cp1250::encode(&repertoire).unwrap();
    assert_eq!(cp1250::decode(&bytes).unwrap(), repertoire);
    assert!(!repertoire.chars().any(char::is_control));
}
#[test]
fn text_never_injects_control_separators_or_substitutes_glyphs() {
    assert_eq!(
        encode_body("LNP\x1dOAŁódź\x1eBx\x1fA1").unwrap(),
        b"LNP\x1dOA\xA3\xF3d\x9F\x1eBx\x1fA1"
    );
    assert!(matches!(encode_body("LOR\x02"), Err(Error::Invalid(_))));
}
