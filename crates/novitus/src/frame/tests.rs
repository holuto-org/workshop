use super::*;

#[test]
fn checksum_matches_vendor_example() {
    // NOVITUS v1.15 §1.2 and §4.4.16: https://novitus.pl/file/get/695
    assert_eq!(encode(b"0#i100/"), b"\x1bP0#i100/9B\x1b\\");
    assert_eq!(encode(b"210#s"), b"\x1bP210#s9C\x1b\\");
}

#[test]
fn frame_round_trip_rejects_bad_checksum() {
    assert_eq!(decode(b"\x1bP210#s9C\x1b\\").unwrap(), b"210#s");
    for invalid in [
        b"\x1bP210#s9D\x1b\\".as_slice(),
        b"\x1bP210#s9c\x1b\\",
        b"\x1bP210#s\x1b\\",
        b"\x1bP9C\x1b\\",
        b"210#s9C",
    ] {
        assert!(
            matches!(decode(invalid), Err(Error::Protocol(_))),
            "{invalid:?}"
        );
    }
}

#[test]
fn maximum_frame_size_and_embedded_escape_are_enforced() {
    let body = vec![b'A'; MAX_FRAME - 6];
    let maximum = encode(&body);
    assert_eq!(maximum.len(), MAX_FRAME);
    assert_eq!(decode(&maximum).unwrap(), body);
    for invalid in [
        encode(&vec![b'A'; MAX_FRAME - 5]),
        encode(b"1#Ubad\x1btext"),
        b"\x1bP\x1b\\".to_vec(),
        b"\x1bQ210#s9C\x1b\\".to_vec(),
        b"\x1bP210#s9C\x1b/".to_vec(),
    ] {
        assert!(matches!(decode(&invalid), Err(Error::Protocol(_))));
    }
}

#[test]
fn every_single_bit_corruption_of_a_checked_body_is_rejected() {
    let raw = b"\x1bP0#i100/9B\x1b\\";
    for position in 2..raw.len() - 4 {
        for bit in 0..8 {
            let mut corrupt = raw.to_vec();
            corrupt[position] ^= 1 << bit;
            assert!(
                matches!(decode(&corrupt), Err(Error::Protocol(_))),
                "{position}:{bit}"
            );
        }
    }
}
