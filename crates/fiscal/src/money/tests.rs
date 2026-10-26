use proptest::prelude::*;

use super::{Grosze, zloty};
use crate::error::Error;

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
    for (text, message) in [
        (
            "-0.01",
            "Kwota musi być zapisana cyframi, z najwyżej 2 miejscami po przecinku.",
        ),
        (
            "1.001",
            "Kwota musi być zapisana cyframi, z najwyżej 2 miejscami po przecinku.",
        ),
        (
            "1e2",
            "Kwota musi być zapisana cyframi, z najwyżej 2 miejscami po przecinku.",
        ),
        (
            "100000000",
            "Kwota musi być nieujemna i mieścić się w zakresie drukarki.",
        ),
        (
            "18446744073709551616",
            "Kwota musi być zapisana cyframi, z najwyżej 2 miejscami po przecinku.",
        ),
    ] {
        let error = Grosze::parse(text).unwrap_err();
        assert!(matches!(error, Error::Invalid(_)), "{text}");
        assert_eq!(error.to_string(), message, "{text}");
    }
    assert_eq!(zloty(0), "0,00 zł");
    assert_eq!(zloty(123_456_789), "1 234 567,89 zł");
    assert_eq!(Grosze::MAX.to_string(), "99 999 999,99 zł");
    assert_eq!(zloty(u64::MAX), "184 467 440 737 095 516,15 zł");
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

#[test]
fn money_constructor_and_checked_addition_enforce_the_same_upper_boundary() {
    for value in [10_000_000_000, u64::MAX] {
        assert_eq!(
            Grosze::new(value).unwrap_err().to_string(),
            "Kwota musi być nieujemna i mieścić się w zakresie drukarki."
        );
    }
    assert_eq!(
        Grosze::MAX.checked_add(Grosze::ZERO).unwrap().get(),
        9_999_999_999
    );
    assert_eq!(
        Grosze::MAX
            .checked_add(Grosze::new(1).unwrap())
            .unwrap_err()
            .to_string(),
        "Kwota musi być nieujemna i mieścić się w zakresie drukarki."
    );
    let overflow = [Grosze::MAX, Grosze::new(1).unwrap()]
        .into_iter()
        .sum::<Grosze>();
    assert!(serde_json::from_str::<Grosze>(&serde_json::to_string(&overflow).unwrap()).is_err());
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

    #[test]
    fn collection_sums_never_wrap_or_hide_an_out_of_range_total(
        amounts in prop::collection::vec(
            prop_oneof![Just(0), Just(9_999_999_999), 0..=9_999_999_999_u64], 0..100,
        ),
    ) {
        let expected = amounts.iter().map(|amount| u128::from(*amount)).sum::<u128>();
        let actual = amounts.into_iter().map(|amount| Grosze::new(amount).unwrap()).sum::<Grosze>();
        prop_assert_eq!(u128::from(actual.get()), expected.min(10_000_000_000));
    }
}
