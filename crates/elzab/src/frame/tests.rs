use super::*;
use std::iter::once;

#[test]
fn crc_and_encoded_requests_match_independent_reference_vectors() {
    assert_eq!(crc(b"123456789"), 0xBB3D);
    assert_eq!(encode("1", b"LOR").unwrap(), b"\x021\x1cLOR4E0E\x03");
    assert_eq!(encode("1", b"XTR").unwrap(), b"\x021\x1cXTRBA44\x03");
    assert_eq!(decode(b"\x021\x1cXTRBA44\x03").unwrap(), ("1", &b"XTR"[..]));
}

#[test]
fn single_bit_damage_in_framing_header_or_data_is_rejected() {
    let vector = b"\x021\x1cXTRBA44\x03";
    for index in (0..6).chain(once(vector.len() - 1)) {
        for bit in 0..8 {
            let mut damaged = vector.to_vec();
            damaged[index] ^= 1 << bit;
            assert!(
                matches!(decode(&damaged), Err(Error::Protocol(message))
                if message == "Nieprawidłowa ramka ElzabSTX."),
                "byte {index}, bit {bit}"
            );
        }
    }
}

#[test]
fn frame_boundaries_preserve_binary_data_and_reject_overflow() {
    let data = vec![b'A'; 1016];
    let raw = encode("1", &data).unwrap();
    assert_eq!(raw.len(), MAX_FRAME);
    assert_eq!(decode(&raw).unwrap(), ("1", data.as_slice()));
    let mut oversized = raw;
    oversized.insert(2, b'A');
    assert!(matches!(decode(&oversized), Err(Error::Protocol(_))));
    assert!(matches!(
        encode("1", &vec![b'A'; 1017]),
        Err(Error::Protocol(_))
    ));

    let data: Vec<_> = (0..=255).filter(|b| !matches!(*b, STX | ETX)).collect();
    let raw = encode("1\x1dALL\x1d1\x1d1\x1dNDT", &data).unwrap();
    assert_eq!(
        decode(&raw).unwrap(),
        ("1\x1dALL\x1d1\x1d1\x1dNDT", data.as_slice())
    );
    assert_eq!(decode(&encode("", b"").unwrap()).unwrap(), ("", &b""[..]));
}

#[test]
fn malformed_boundaries_headers_and_embedded_framing_are_rejected() {
    for (header, data) in [
        ("1\x1c", &b"XTR"[..]),
        ("Ł", b"XTR"),
        ("1\x02", b"XTR"),
        ("1\x03", b"XTR"),
        ("1", b"X\x02TR"),
        ("1", b"X\x03TR"),
    ] {
        assert!(matches!(encode(header, data), Err(Error::Protocol(_))));
    }
    for raw in [&b""[..], b"\x02\x1c0000\x03", b"\x021XTR0000\x03"] {
        assert!(matches!(decode(raw), Err(Error::Protocol(_))));
    }
    for content in [
        &b"1XTR"[..],
        b"\x80\x1cXTR",
        b"1\x1cX\x02TR",
        b"1\x1cX\x03TR",
    ] {
        let mut raw = vec![STX];
        raw.extend_from_slice(content);
        raw.extend_from_slice(format!("{:04X}", crc(content)).as_bytes());
        raw.push(ETX);
        assert!(matches!(decode(&raw), Err(Error::Protocol(_))));
    }
}
