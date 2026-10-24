use serde_json::json;
use std::error::Error as StdError;

use super::BuyerNip;
use crate::Error;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn buyer_nip_preserves_digits_without_accepting_separators_or_unicode_lookalikes()
-> Result<(), Box<dyn StdError>> {
    assert_eq!(
        serde_json::to_value(BuyerNip::parse(" 5260250274 ")?)?,
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
        assert!(
            matches!(
                BuyerNip::parse(invalid), Err(Error::Invalid(message))
                    if message == "NIP nabywcy musi zawierać 10 cyfr."
            ),
            "invalid fiscal identifier receives the operator-facing validation error"
        );
    }
    Ok(())
}
