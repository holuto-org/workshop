use super::{format, parse};

#[test]
fn plain_decimals_parse_exactly_at_the_scale() {
    for (text, units) in [
        ("0", 0),
        ("12", 1200),
        ("12.5", 1250),
        ("12.50", 1250),
        ("0.01", 1),
        ("007.10", 710),
        ("184467440737095516.15", u64::MAX),
    ] {
        assert_eq!(parse(text, 2), Some(units), "{text}");
    }
    assert_eq!(parse("1.00000001", 8), Some(100_000_001));
}

#[test]
fn anything_but_digits_and_one_point_is_rejected() {
    for text in [
        "",
        ".",
        ".5",
        "5.",
        "1.001",
        "-1",
        "+1",
        "1e2",
        "1,5",
        " 1",
        "1 ",
        "1_000",
        "1.2.3",
        "NaN",
        "Infinity",
        "١",
        "184467440737095516.16",
    ] {
        assert_eq!(parse(text, 2), None, "{text:?}");
    }
}

#[test]
fn formatting_drops_only_trailing_fraction_zeros() {
    for (units, scale, text) in [
        (0, 8, "0"),
        (150_000_000, 8, "1.5"),
        (1, 8, "0.00000001"),
        (1_000_000_000_000, 8, "10000"),
        (2300, 2, "23"),
        (550, 2, "5.5"),
        (u64::MAX, 2, "184467440737095516.15"),
    ] {
        assert_eq!(format(units, scale), text);
        assert_eq!(parse(text, scale), Some(units), "{text}");
    }
}
