use proptest::prelude::*;

use super::{Grosze, zloty};
use crate::Error;

#[test]
fn printer_money_has_exact_nonnegative_grosze_and_a_hard_upper_bound() {
    for (text, expected) in [
        ("0", 0),
        ("0.01", 1),
        ("12.5", 1250),
        ("99999999.99", 9_999_999_999),
    ] {
        assert_eq!(Grosze::parse(text).unwrap().get(), expected, "{text}");
    }
    for text in ["-0.01", "1.001", "1e2", "100000000", "18446744073709551616"] {
        assert!(
            matches!(Grosze::parse(text), Err(Error::Invalid(_))),
            "{text}"
        );
    }
    assert_eq!(zloty(0), "0,00 zł");
    assert_eq!(zloty(123_456_789), "1 234 567,89 zł");
    assert_eq!(Grosze::MAX.to_string(), "99 999 999,99 zł");
    assert_eq!(Grosze::ZERO.checked_sub(Grosze::new(1).unwrap()), None);
    assert_eq!(
        [Grosze::MAX, Grosze::MAX].into_iter().sum::<Grosze>().get(),
        10_000_000_000
    );
}

#[test]
fn persisted_money_is_revalidated_when_loaded() {
    assert_eq!(
        serde_json::from_str::<Grosze>("9999999999").unwrap(),
        Grosze::MAX
    );
    assert!(
        serde_json::from_str::<Grosze>("10000000000").is_err(),
        "out of range journal amount"
    );
    assert!(
        serde_json::from_str::<Grosze>("-1").is_err(),
        "negative journal amount"
    );
}

proptest! {
    #[test]
    fn addition_and_sums_match_bounded_integer_arithmetic(
        left in 0..=9_999_999_999_u64,
        right in 0..=9_999_999_999_u64,
    ) {
        let a = Grosze::new(left).unwrap();
        let b = Grosze::new(right).unwrap();
        let expected = left + right;
        prop_assert_eq!(
            a.checked_add(b).ok().map(Grosze::get),
            (expected <= 9_999_999_999).then_some(expected)
        );
        prop_assert_eq!([a,b].into_iter().sum::<Grosze>().get(), expected.min(10_000_000_000));
        prop_assert_eq!(a.checked_sub(b).map(Grosze::get), left.checked_sub(right));
    }
}
