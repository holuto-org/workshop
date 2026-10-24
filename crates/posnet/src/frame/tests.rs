use super::{decode, encode, wrap};
use crate::{Error, text};

pub(crate) fn reply(payload: &str) -> Vec<u8> {
    wrap(&text::encode(payload).unwrap())
}

#[test]
fn published_crc_vector() {
    // Spec p. 13 worked example.
    assert_eq!(
        encode("trinit", &[("bm", "0".to_owned())]).unwrap(),
        b"\x02trinit\tbm0\t#4825\x03"
    );
}

#[test]
fn polish_text_is_encoded_before_the_checksum() {
    let frame = encode(
        "trline",
        &[("na", "Koło".to_owned()), ("vt", "0".to_owned())],
    )
    .unwrap();
    assert!(frame.windows(4).any(|window| window == b"Ko\xB3o"));
    let (command, reply) = decode(&frame).unwrap();
    assert_eq!(command, "trline");
    assert_eq!(reply.get("na"), Some("Koło"));
    assert_eq!(reply.get("vt"), Some("0"));
}

#[test]
fn command_values_reject_separators_and_unprintable_text() {
    for value in ["x\ty", "x\u{3}", "x\n", "🔧"] {
        assert!(
            matches!(
                encode("trline", &[("na", value.to_owned())]),
                Err(Error::Invalid(_))
            ),
            "{value:?}"
        );
    }
    assert!(encode("TRINIT", &[]).is_err());
    assert!(encode("trline", &[("name", "x".to_owned())]).is_err());
}

#[test]
fn corrupt_and_ambiguous_replies_are_rejected() {
    for frame in [
        b"\x02trinit\tbm0\t#0000\x03".to_vec(),
        reply("trinit\tbm0\tbm1\t"),
        reply("trinit\tb\t"),
        b"\x02trinit\tbm0\t#48\x03".to_vec(),
        b"trinit\tbm0\t#4825\x03".to_vec(),
        wrap(b"scnt\tbt\x81\t"),
    ] {
        assert!(
            matches!(decode(&frame), Err(Error::Protocol(_))),
            "{frame:?}"
        );
    }
}

#[test]
fn an_empty_value_is_still_a_parameter() {
    let (_, reply) = decode(&reply("scnt\tbt\t")).unwrap();
    assert_eq!(reply.get("bt"), Some(""));
}

#[test]
fn both_documented_error_forms_are_device_errors() {
    for payload in ["trline\t?2000", "ERR\t?2000\tcmtrline\tfdvt\t"] {
        let Err(Error::Device { code, command, .. }) = decode(&reply(payload)) else {
            panic!("{payload:?} is not a device error");
        };
        assert_eq!((code, command.as_str()), (2000, "trline"));
    }
    assert!(matches!(decode(&reply("ERR\t")), Err(Error::Protocol(_))));
}
