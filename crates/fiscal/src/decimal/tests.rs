use proptest::prelude::*;

use super::{format, parse};

#[test]
fn only_unsigned_plain_decimals_at_the_requested_precision_are_accepted() {
    for (name, text, scale, expected) in [
        ("whole", "12", 2, Some(1200)),
        ("leading zeros", "00012.5", 2, Some(1250)),
        ("zero", "0", 0, Some(0)),
        ("max", "18446744073709551615", 0, Some(u64::MAX)),
        ("overflow", "18446744073709551616", 0, None),
        ("scaling overflow", "18446744073709551615", 1, None),
        ("too precise", "12.500", 2, None),
        ("empty", "", 2, None),
        ("fraction only", ".5", 2, None),
        ("trailing point", "1.", 2, None),
        ("negative", "-1", 2, None),
        ("positive sign", "+1", 2, None),
        ("space", " 1", 2, None),
        ("grouped", "1_000", 2, None),
        ("exponent", "1e2", 2, None),
        ("comma", "1,25", 2, None),
        ("second point", "1.2.3", 4, None),
        ("unicode digit", "١", 2, None),
    ] {
        assert_eq!(parse(text, scale), expected, "{name}");
    }
    assert_eq!(format(1200, 2), "12");
    assert_eq!(format(1250, 2), "12.5");
    assert_eq!(format(1, 8), "0.00000001");
}

proptest! {
    #[test]
    fn parsing_matches_an_independent_integer_model(
        whole in any::<u64>(),
        fraction in 0..100_000_000_u64,
        scale in 0..=8_usize,
    ) {
        let unit = 10_u128.pow(u32::try_from(scale).unwrap());
        let remainder = u128::from(fraction) % unit;
        let text = if scale == 0 {
            whole.to_string()
        } else {
            format!("{whole}.{remainder:0scale$}")
        };
        let model = u64::try_from(u128::from(whole) * unit + remainder).ok();
        prop_assert_eq!(parse(&text, scale), model);
    }

    #[test]
    fn formatting_is_exact_for_every_scaled_integer(units in any::<u64>(), scale in 0..=8_usize) {
        let text = format(units, scale);
        prop_assert_eq!(parse(&text, scale), Some(units));
        prop_assert!(!text.ends_with('.') && (!text.contains('.') || !text.ends_with('0')));
    }
}
