use rust_decimal::Decimal;

use super::{Grosze, half_up};

#[test]
fn amounts_print_the_polish_way() {
    assert_eq!(Grosze::new(3468).unwrap().to_string(), "34,68 zł");
    assert_eq!(Grosze::new(5).unwrap().to_string(), "0,05 zł");
    assert_eq!(
        Grosze::new(123_456_789).unwrap().to_string(),
        "1 234 567,89 zł"
    );
    assert_eq!(Grosze::MAX.to_string(), "99 999 999,99 zł");
}

#[test]
fn only_whole_nonnegative_grosze_within_the_printer_range_convert() {
    assert_eq!(
        Grosze::from_zloty(Decimal::new(25_000, 2)).unwrap().get(),
        25_000
    );
    for zloty in [
        "0.001",
        "-1.00",
        "100000000.00",
        "79228162514264337593543950335",
    ] {
        assert!(
            Grosze::from_zloty(zloty.parse().unwrap()).is_err(),
            "{zloty}"
        );
    }
    assert!(Grosze::new(Grosze::MAX.get() + 1).is_err());
}

#[test]
fn an_oversized_sum_exceeds_the_range_instead_of_wrapping() {
    let sum: Grosze = [Grosze::MAX, Grosze::MAX].into_iter().sum();
    assert!(sum > Grosze::MAX);
}

#[test]
fn halves_round_away_from_zero_like_the_printer() {
    assert_eq!(half_up(Decimal::new(25, 1)), Decimal::from(3));
    assert_eq!(half_up(Decimal::new(35, 1)), Decimal::from(4));
    assert_eq!(half_up(Decimal::new(49_995, 3)), Decimal::from(50));
}
