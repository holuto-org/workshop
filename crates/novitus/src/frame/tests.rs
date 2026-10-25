use super::*;

#[test]
fn checksum_matches_vendor_example() {
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
fn identity_query_frame_snapshot() {
    // N:L1230-L1290: identity requests explicitly omit the checksum.
    insta::assert_debug_snapshot!(query(b"30#v"), @"
    [
        27,
        80,
        51,
        48,
        35,
        118,
        27,
        92,
    ]
    ");
}
