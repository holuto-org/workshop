use super::{Grosze, zloty};

#[test]
fn amounts_print_the_polish_way() {
    assert_eq!(Grosze::new(3468).unwrap().to_string(), "34,68 zł");
    assert_eq!(Grosze::new(5).unwrap().to_string(), "0,05 zł");
    assert_eq!(
        Grosze::new(123_456_789).unwrap().to_string(),
        "1 234 567,89 zł"
    );
    assert_eq!(Grosze::MAX.to_string(), "99 999 999,99 zł");
    assert_eq!(zloty(10_000_000_000), "100 000 000,00 zł");
}

#[test]
fn only_whole_nonnegative_grosze_within_the_printer_range_parse() {
    assert_eq!(Grosze::parse("250.00").unwrap().get(), 25_000);
    assert_eq!(Grosze::parse("99999999.99").unwrap(), Grosze::MAX);
    for zloty in [
        "0.001",
        "-1.00",
        "100000000.00",
        "79228162514264337593543950335",
        "NaN",
    ] {
        assert!(Grosze::parse(zloty).is_err(), "{zloty}");
    }
    assert!(Grosze::new(Grosze::MAX.get() + 1).is_err());
}

#[test]
fn an_oversized_sum_exceeds_the_range_instead_of_wrapping() {
    let sum: Grosze = [Grosze::MAX, Grosze::MAX].into_iter().sum();
    assert!(sum > Grosze::MAX);
}
