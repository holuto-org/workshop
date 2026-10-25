use super::*;
#[test]
fn crc_matches_reference_vectors() {
    assert_eq!(crc(b"123456789"), 0xBB3D);
    assert_eq!(crc(b"1\x1cLOR"), 0x4E0E);
    assert_eq!(crc(b"1\x1cXTR"), 0xBA44);
}
#[test]
fn frame_header_and_separators_round_trip() {
    let raw = encode("1\x1dALL\x1d1\x1d1\x1dNDT", b"LIT\x1dGAOlej\x1eBA").unwrap();
    assert_eq!(
        decode(&raw).unwrap(),
        ("1\x1dALL\x1d1\x1d1\x1dNDT", &b"LIT\x1dGAOlej\x1eBA"[..])
    );
    assert_eq!(encode("1", b"LOR").unwrap(), b"\x021\x1cLOR4E0E\x03");
}
#[test]
fn malformed_frames_and_overflow_are_rejected() {
    for raw in [
        &b"\x021\x1cLOR0000\x03"[..],
        b"\x021\x1cLOR4E0E\x04",
        b"\x02x\x1cXTRBA44\x03",
    ] {
        assert!(decode(raw).is_err());
    }
    assert!(encode("1\x1c", b"LOR").is_err());
    assert!(encode("1", b"LOR\x02").is_err());
    assert!(encode("1", &vec![b'A'; 1017]).is_err());
}

#[test]
fn empty_default_header_and_empty_reply_data_round_trip() {
    let raw = encode("", b"").unwrap();
    assert_eq!(raw.len(), 7);
    assert_eq!(decode(&raw).unwrap(), ("", &b""[..]));
}
