use super::BuyerNip;

#[test]
fn a_buyer_nip_is_exactly_ten_digits() {
    assert_eq!(
        BuyerNip::parse(" 5260250274 ").unwrap().as_str(),
        "5260250274"
    );
    for invalid in [
        "",
        "526025027",
        "52602502741",
        "526-025-02-74",
        "526025027a",
        "５２６０２５０２７４",
    ] {
        assert!(BuyerNip::parse(invalid).is_err(), "{invalid}");
    }
}
