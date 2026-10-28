use proptest::prelude::*;

use super::{MAX_FRAME, crc16, decode, encode, wrap};
use fiscal::{cp1250, error::Error};

pub(crate) fn reply(payload: &str) -> Vec<u8> {
    wrap(&cp1250::encode(payload).unwrap())
}

#[test]
fn published_checksums_cover_the_payload_not_the_delimiters() {
    assert_eq!(crc16(b"123456789"), 0x31C3);
    assert_eq!(
        encode("trinit", &[("bm", "0".to_owned())]).unwrap(),
        b"\x02trinit\tbm0\t#4825\x03"
    );
    assert_eq!(
        encode("trline", &[("na", "ł".to_owned())]).unwrap(),
        b"\x02trline\tna\xB3\t#CA09\x03"
    );
    assert_eq!(
        encode("trline", &[("na", "Zażółć gęślą jaźń".to_owned())]).unwrap(),
        b"\x02trline\tnaZa\xBF\xF3\xB3\xE6 g\xEA\x9Cl\xB9 ja\x9F\xF1\t#CF2E\x03"
    );
}

#[test]
fn lowercase_hex_and_an_empty_parameter_are_valid_replies() {
    let mut bytes = reply("scnt\tbt\t");
    bytes.make_ascii_lowercase();
    let (command, fields) = decode(&bytes).unwrap();
    assert_eq!(command, "scnt");
    assert_eq!(fields.get("bt"), Some(""));
}

#[test]
fn documented_device_errors_preserve_the_command_code_and_field() {
    for (payload, expected_rejected) in [
        ("trinit\t?2036\tfdts\t", false),
        ("ERR\t?2036\tcmtrinit\tfdts\t", true),
    ] {
        let error = decode(&reply(payload)).unwrap_err();
        assert!(
            matches!(&error,
                Error::Device { code:2036, command, detail, rejected }
                if command == "trinit"
                    && detail.to_string() == "Drukarka zwróciła błąd 2036 (trinit, parametr ts). Urządzenie w stanie tylko do odczytu."
                    && *rejected == expected_rejected),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            "Drukarka zwróciła błąd 2036 (trinit, parametr ts). \
             Urządzenie w stanie tylko do odczytu."
        );
    }
}

#[test]
fn corrupt_ambiguous_or_incomplete_replies_never_become_success() {
    for (name, bytes) in [
        ("crc", b"\x02scnt\tbt1\t#0000\x03".to_vec()),
        ("unassigned cp1250", wrap(b"scnt\tbt\x81\t")),
        ("duplicate field", reply("scnt\tbt1\tbt2\t")),
        ("duplicate token", reply("scnt\t@0001\t@0002\t")),
        ("duplicate error", reply("ERR\t?2000\t?2036\t")),
        ("short field", reply("scnt\tb\t")),
        ("nonnumeric error", reply("ERR\t?bad\t")),
        ("missing error", reply("ERR\tcmtrinit\t")),
        ("short crc", b"\x02scnt\t#123\x03".to_vec()),
        ("missing stx", b"scnt\t#1234\x03".to_vec()),
    ] {
        assert!(matches!(decode(&bytes), Err(Error::Protocol(_))), "{name}");
    }
}

#[test]
fn command_validation_rejects_frame_injection_and_unencodable_text() {
    for (name, command, key, value) in [
        ("tab", "trline", "na", "a\tb"),
        ("etx", "trline", "na", "\u{3}"),
        ("newline", "trline", "na", "a\nb"),
        ("emoji", "trline", "na", "😀"),
        ("uppercase command", "TRINIT", "bm", "0"),
        ("long key", "trinit", "bad", "0"),
    ] {
        assert!(
            matches!(
                encode(command, &[(key, value.to_owned())]),
                Err(Error::Invalid(_))
            ),
            "{name}"
        );
    }
}

#[test]
fn frame_size_is_bounded_on_both_sides_of_the_connection() {
    // 7 envelope bytes + "x\tna" + value + TAB.
    let largest = "a".repeat(MAX_FRAME - 12);
    let frame = encode("x", &[("na", largest.clone())]).unwrap();
    assert_eq!(frame.len(), MAX_FRAME);
    assert_eq!(decode(&frame).unwrap().1.get("na"), Some(largest.as_str()));
    assert!(matches!(
        encode("x", &[("na", format!("{largest}a"))]),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        decode(&reply(&format!("x\tna{largest}a\t"))),
        Err(Error::Protocol(_))
    ));
}

proptest! {
    #[test]
    fn printable_payloads_round_trip_with_exact_field_boundaries(
        command in "!?[a-z][a-z0-9]{0,15}",
        value in "[ -~ąćęłńóśźżĄĆĘŁŃÓŚŹŻ€–]{0,200}",
    ) {
        let frame = encode(&command, &[("na", value.clone())]).unwrap();
        let (answered, fields) = decode(&frame).unwrap();
        prop_assert_eq!(answered, command);
        prop_assert_eq!(fields.fields(), &[("na".to_owned(), value)]);
    }
}
