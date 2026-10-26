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
    assert_eq!(
        format!("{:?}", BuyerNip::parse("5260250274")?),
        "BuyerNip([REDACTED])"
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

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Document identity and queue validation are public boundaries."
)]
fn document_identity_ignores_display_changes_but_not_the_allocation_or_kind()
-> Result<(), Box<dyn StdError>> {
    use super::Document;
    use crate::api::queue_form;
    use uuid::Uuid;
    let original: Document = serde_json::from_value(json!({
        "kind":"advance","orderId":Uuid::from_u128(10),"orderNumber":42,
        "allocationId":Uuid::from_u128(12),"amount":"12.34","customerName":null,
        "occurredAt":"2026-10-24T10:00:00Z","dueAt":"2026-10-24T10:00:00Z",
        "latestAt":"2026-10-24T10:00:00Z","isOverdue":false
    }))?;
    assert_eq!(
        format!("{original:?}"),
        "Document { kind: Advance, order_id: 00000000-0000-0000-0000-00000000000a, .. }"
    );
    let mut updated = original.clone();
    updated.order_number = 43;
    updated.amount = 9999;
    updated.customer_name = Some("Nowa nazwa".to_owned());
    updated.is_overdue = true;
    assert!(original.same_document(&updated));
    updated.allocation_id = Some(Uuid::from_u128(13));
    assert!(!original.same_document(&updated));
    updated.allocation_id = original.allocation_id;
    updated.kind = super::ReceiptKind::Sale;
    assert!(!original.same_document(&updated));
    for (kind, allocation, number, expected) in [
        (
            super::ReceiptKind::Advance,
            None,
            42,
            "zaliczka bez identyfikatora alokacji",
        ),
        (
            super::ReceiptKind::Sale,
            original.allocation_id,
            42,
            "sprzedaż z identyfikatorem alokacji",
        ),
        (
            super::ReceiptKind::Advance,
            original.allocation_id,
            0,
            "numer zlecenia musi być dodatni",
        ),
        (
            super::ReceiptKind::Advance,
            original.allocation_id,
            -1,
            "numer zlecenia musi być dodatni",
        ),
    ] {
        let document = Document {
            kind,
            order_number: number,
            allocation_id: allocation,
            ..original.clone()
        };
        assert!(
            matches!(queue_form(&document,None),Err(Error::Invalid(message))
            if message == format!("Nieprawidłowy dokument: {expected}."))
        );
    }
    Ok(())
}
