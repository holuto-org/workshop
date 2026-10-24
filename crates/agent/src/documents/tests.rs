use serde_json::json;

use super::BuyerNip;

#[test]
fn a_buyer_nip_is_exactly_ten_digits() {
    assert_eq!(
        serde_json::to_value(BuyerNip::parse(" 5260250274 ").unwrap()).unwrap(),
        json!("5260250274")
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
