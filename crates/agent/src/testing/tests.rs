use std::error::Error as StdError;

use serde_json::{Value, json};
use uuid::Uuid;

use super::{ALLOCATION_ID, Holuto, JOB_ID, ORDER_ID, PRINTER_ID, job_json, with};
use crate::{
    api::{
        Api, ApiError,
        model::{Job, Telemetry},
        queue_form,
    },
    documents::Document,
};

type TestResult = Result<(), Box<dyn StdError>>;
const OTHER_ALLOCATION: Uuid = Uuid::from_u128(13);
const OTHER_ORDER: Uuid = Uuid::from_u128(14);
const OTHER_PRINTER: Uuid = Uuid::from_u128(15);
const STATES: [&str; 6] = ["queued", "taken", "done", "failed", "expired", "unknown"];

fn api(server: &Holuto) -> Result<Api, Box<dyn StdError>> {
    let mut api = Api::new(server.url(), server.token(), "DEMO0000001")?;
    api.session(&telemetry())?;
    Ok(api)
}

fn telemetry() -> Telemetry {
    super::telemetry("DEMO0000001")
}

fn document(order: Uuid, allocation: Option<Uuid>, occurred_at: &str) -> Value {
    json!({
        "kind": if allocation.is_some() { "advance" } else { "sale" },
        "orderId": order, "orderNumber": 10, "allocationId": allocation,
        "amount": "250.00", "customerName": null, "occurredAt": occurred_at
    })
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup and HTTP failures use Result"
)]
fn queue_refuses_unsupported_capabilities_and_quantity_precision_without_creating_work()
-> TestResult {
    for (advances, zero, precision, changes, code) in [
        (
            false,
            true,
            8,
            json!({
                "advances": [{"vatSlot": 0, "vatRate": "23", "amount": "100.00"}],
                "payments": [{"form": "card", "amount": "150.00"}]
            }),
            "fiscal_printer_advances_unsupported",
        ),
        (
            true,
            false,
            8,
            json!({
                "advances": [
                    {"vatSlot": 0, "vatRate": "23", "amount": "200.00"},
                    {"vatSlot": 1, "vatRate": "8", "amount": "50.00"}
                ], "payments": []
            }),
            "fiscal_printer_zero_total_unsupported",
        ),
        (
            true,
            true,
            3,
            json!({
                "lines": [{"name": "Usługa", "quantity": "1.0001", "unitPrice": "250.00",
                    "total": "250.03", "vatRate": "23", "vatSlot": 0}],
                "total": "250.03", "payments": [{"form": "card", "amount": "250.03"}]
            }),
            "fiscal_printer_quantity_precision",
        ),
    ] {
        let server = Holuto::new();
        server.queue_job(with(job_json(), &changes));
        let mut telemetry = telemetry();
        telemetry.capabilities.advance_deductions = advances;
        telemetry.capabilities.zero_total_receipts = zero;
        telemetry.max_quantity_decimals = precision;
        let mut api = Api::new(server.url(), server.token(), "DEMO0000001")?;
        api.session(&telemetry)?;
        let document: Document =
            serde_json::from_value(document(ORDER_ID, None, "2026-10-16T10:00:00Z"))?;
        let result = api.queue(&queue_form(&document, None)?);
        assert!(
            matches!(&result,
                Err(ApiError::Status { status: 409, detail: Some(detail) }) if detail == code),
            "{result:?}"
        );
        assert!(matches!(api.job(Uuid::from_u128(100)),
                Err(ApiError::Status { status: 404, detail: Some(detail) })
                    if detail == "fiscal_not_found"));
        assert!(api.take()?.is_none());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup and HTTP failures use Result"
)]
fn queue_precision_ignores_trailing_zeros_and_accepts_the_device_boundary() -> TestResult {
    for quantity in ["1.123000", "1.123"] {
        let server = Holuto::new();
        server.queue_job(with(
            job_json(),
            &json!({
                "lines": [{"name": "Usługa", "quantity": quantity, "unitPrice": "100.00",
                    "total": "112.30", "vatRate": "23", "vatSlot": 0}],
                "total": "112.30", "payments": [{"form": "card", "amount": "112.30"}]
            }),
        ));
        let mut telemetry = telemetry();
        telemetry.max_quantity_decimals = 3;
        let mut api = Api::new(server.url(), server.token(), "DEMO0000001")?;
        api.session(&telemetry)?;
        let document: Document =
            serde_json::from_value(document(ORDER_ID, None, "2026-10-16T10:00:00Z"))?;
        api.queue(&queue_form(&document, None)?)?;
        let job = api.take()?.ok_or("missing queued job")?;
        assert_eq!(job.lines.first().ok_or("missing line")?.quantity, quantity);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup and HTTP failures use Result"
)]
#[expect(
    clippy::too_many_lines,
    reason = "literal backend conflict matrix and refused-write evidence stay visible together"
)]
fn queue_conflicts_follow_kind_allocation_state_and_paired_printer() -> TestResult {
    // ../backend/crates/fiscal/queries/receipts/queue_blocked_job.sql:8-35.
    for (
        name,
        existing_order,
        existing_printer,
        existing_allocation,
        requested_allocation,
        blocked,
    ) in [
        (
            "same sale",
            ORDER_ID,
            OTHER_PRINTER,
            None,
            None,
            [true, true, true, false, false, true],
        ),
        (
            "same advance",
            ORDER_ID,
            OTHER_PRINTER,
            Some(ALLOCATION_ID),
            Some(ALLOCATION_ID),
            [true, true, true, false, false, true],
        ),
        (
            "other advance allocation",
            ORDER_ID,
            OTHER_PRINTER,
            Some(OTHER_ALLOCATION),
            Some(ALLOCATION_ID),
            [false; 6],
        ),
        (
            "advance before sale",
            ORDER_ID,
            OTHER_PRINTER,
            Some(ALLOCATION_ID),
            None,
            [true, true, false, false, false, true],
        ),
        (
            "sale before advance",
            ORDER_ID,
            OTHER_PRINTER,
            None,
            Some(ALLOCATION_ID),
            [true, true, true, false, false, true],
        ),
        (
            "other order paired printer",
            OTHER_ORDER,
            PRINTER_ID,
            None,
            None,
            [false, false, false, false, false, true],
        ),
        (
            "other order other printer",
            OTHER_ORDER,
            OTHER_PRINTER,
            None,
            None,
            [false; 6],
        ),
    ] {
        for (state, blocked) in STATES.into_iter().zip(blocked) {
            let server = Holuto::new();
            let existing = with(
                job_json(),
                &json!({
                    "orderId": existing_order, "printerId": existing_printer,
                    "kind": if existing_allocation.is_some() { "advance" } else { "sale" },
                    "allocationId": existing_allocation, "state": state
                }),
            );
            server.insert(existing.clone());
            let api = api(&server)?;
            let document: Document = serde_json::from_value(document(
                ORDER_ID,
                requested_allocation,
                "2026-10-16T10:00:00Z",
            ))?;
            let result = api.queue(&queue_form(&document, None)?);
            if blocked {
                assert!(
                    matches!(
                        &result,
                        Err(ApiError::Status { status: 409, detail: Some(detail) })
                            if detail == "fiscal_job_conflict"
                    ),
                    "{name}/{state}: expected exact fiscal_job_conflict, got {result:?}"
                );
                assert!(
                    matches!(
                        &api.job(Uuid::from_u128(101)),
                        Err(ApiError::Status { status: 404, detail: Some(detail) })
                            if detail == "fiscal_not_found"
                    ),
                    "{name}/{state}: refused queue must not create a job"
                );
                assert!(
                    api.take()?.is_none(),
                    "{name}/{state}: refused queue must not enqueue work"
                );
            } else {
                result?;
                let queued = api.job(Uuid::from_u128(101))?;
                let expected: Job = serde_json::from_value(with(
                    job_json(),
                    &json!({
                        "id": Uuid::from_u128(101), "orderId": ORDER_ID,
                        "kind": document.kind, "allocationId": requested_allocation,
                        "state": "queued", "attempt": null, "takenAt": null
                    }),
                ))?;
                assert_eq!(
                    queued, expected,
                    "{name}/{state}: accepted queue must persist the requested document"
                );
            }
            assert_eq!(
                server.job(JOB_ID),
                existing,
                "{name}/{state}: existing evidence must not change"
            );
        }
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup and HTTP failures use Result"
)]
fn documents_exclude_only_the_jobs_that_make_each_document_ineligible() -> TestResult {
    // ../backend/crates/fiscal/queries/receipts/documents.sql:50-64,92-98.
    for (name, existing_order, existing_allocation, document_allocation, hidden) in [
        (
            "same sale",
            ORDER_ID,
            None,
            None,
            [true, true, true, false, false, true],
        ),
        (
            "advance before sale",
            ORDER_ID,
            Some(ALLOCATION_ID),
            None,
            [true, true, false, false, false, true],
        ),
        (
            "sale before advance",
            ORDER_ID,
            None,
            Some(ALLOCATION_ID),
            [true, true, true, false, false, true],
        ),
        (
            "same advance",
            ORDER_ID,
            Some(ALLOCATION_ID),
            Some(ALLOCATION_ID),
            [true, true, true, false, false, true],
        ),
        (
            "other advance allocation",
            ORDER_ID,
            Some(OTHER_ALLOCATION),
            Some(ALLOCATION_ID),
            [false; 6],
        ),
        ("other order sale", OTHER_ORDER, None, None, [false; 6]),
        (
            "other order advance",
            OTHER_ORDER,
            None,
            Some(ALLOCATION_ID),
            [false; 6],
        ),
    ] {
        for (state, hidden) in STATES.into_iter().zip(hidden) {
            let server = Holuto::new();
            let value = document(ORDER_ID, document_allocation, "2026-10-16T10:00:00Z");
            server.documents(vec![value.clone()]);
            server.insert(with(
                job_json(),
                &json!({
                    "orderId": existing_order,
                    "kind": if existing_allocation.is_some() { "advance" } else { "sale" },
                    "allocationId": existing_allocation, "state": state
                }),
            ));
            let page = api(&server)?.documents(None)?;
            let expected = if hidden { json!([]) } else { json!([value]) };
            assert_eq!(
                serde_json::to_value(page)?,
                json!({"items": expected, "nextCursor": null}),
                "{name}/{state}"
            );
        }
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup and HTTP failures use Result"
)]
fn queue_removes_documents_without_skipping_the_cursor_successor() -> TestResult {
    // ../backend/crates/fiscal/queries/receipts/documents.sql:1-26,111-132:
    // eligibility can disappear, but the underlying document still anchors the cursor.
    let server = Holuto::new();
    let mut first_items: Vec<_> = (4000..4099)
        .map(|id| document(Uuid::from_u128(id), None, "2025-10-16T10:00:00Z"))
        .collect();
    let first_advance = document(
        Uuid::from_u128(2000),
        Some(Uuid::from_u128(3000)),
        "2026-10-16T10:00:00Z",
    );
    let next_advance = document(
        Uuid::from_u128(2000),
        Some(Uuid::from_u128(3001)),
        "2026-10-16T10:00:00Z",
    );
    let sale = document(Uuid::from_u128(2000), None, "2026-10-16T10:00:00Z");
    let next_sale = document(Uuid::from_u128(2001), None, "2026-10-16T10:00:00Z");
    first_items.push(first_advance.clone());
    let mut fixtures = first_items.clone();
    fixtures.extend([next_advance.clone(), sale, next_sale.clone()]);
    fixtures.reverse();
    server.documents(fixtures);
    let api = api(&server)?;
    let first = api.documents(None)?;
    assert_eq!(serde_json::to_value(&first.items)?, json!(first_items));
    let cursor = first.next_cursor.ok_or("missing continuation")?;
    assert_eq!(cursor, Uuid::from_u128(3000).to_string());

    api.queue(&queue_form(&serde_json::from_value(first_advance)?, None)?)?;

    let continuation = api.documents(Some(&cursor))?;
    assert_eq!(
        serde_json::to_value(continuation)?,
        json!({
            "items": [next_advance, next_sale], "nextCursor": null
        })
    );
    let _removed = first_items.pop();
    first_items.push(next_advance);
    let refreshed = api.documents(None)?;
    assert_eq!(
        serde_json::to_value(refreshed)?,
        json!({
            "items": first_items, "nextCursor": Uuid::from_u128(3001)
        })
    );
    let missing = api.documents(Some(&Uuid::from_u128(9999).to_string()))?;
    assert_eq!(
        serde_json::to_value(missing)?,
        json!({"items": [], "nextCursor": null})
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn pairing_is_single_use_and_revokes_each_previous_device_credential() -> TestResult {
    let server = Holuto::new();
    server.pairing_code("ABCD1234");
    let first = Api::pair(server.url(), "abcd-1234")?;
    let mut original = Api::new(server.url(), server.token(), "DEMO0000001")?;
    assert!(matches!(
        original.session(&telemetry()),
        Err(ApiError::Unauthorized)
    ));
    let mut paired = Api::new(server.url(), first.token.as_str(), "DEMO0000001")?;
    paired.session(&telemetry())?;
    assert!(matches!(
        Api::pair(server.url(), "ABCD1234"),
        Err(crate::Error::Api(ApiError::Status {
            status: 400, detail: Some(detail),
        })) if detail == "fiscal_pairing_invalid"
    ));
    server.pairing_code("EFGH5678");
    let second = Api::pair(server.url(), "EFGH5678")?;
    assert_eq!(second.token.as_str(), format!("{:0>42}A", 2));
    assert!(matches!(
        paired.session(&telemetry()),
        Err(ApiError::Unauthorized)
    ));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn expired_invalid_and_throttled_pairing_proofs_do_not_revoke_the_token() -> TestResult {
    let server = Holuto::new();
    server.pairing_code("ABCD1234");
    server.expire_pairing_code("ABCD1234");
    for code in ["ABCD1234", "ZZZZZZZZ"] {
        assert!(matches!(
            Api::pair(server.url(), code),
            Err(crate::Error::Api(ApiError::Status {
                status: 400, detail: Some(detail),
            })) if detail == "fiscal_pairing_invalid"
        ));
    }
    server.pairing_code("EFGH5678");
    server.pairing_throttled(true);
    assert!(matches!(
        Api::pair(server.url(), "EFGH5678"),
        Err(crate::Error::Api(ApiError::Status {
            status: 429, detail: Some(detail),
        })) if detail == "rate_limited"
    ));
    let mut original = Api::new(server.url(), server.token(), "DEMO0000001")?;
    original.session(&telemetry())?;
    server.pairing_throttled(false);
    assert_eq!(
        Api::pair(server.url(), "EFGH5678")?.token.as_str(),
        format!("{:0>42}A", 1)
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn pairing_limits_guesses_without_consuming_a_valid_code() -> TestResult {
    let server = Holuto::new();
    server.pairing_code("ABCD1234");
    for _ in 0..10 {
        assert!(matches!(
            Api::pair(server.url(), "ZZZZZZZZ"),
            Err(crate::Error::Api(ApiError::Status { status: 400, .. }))
        ));
    }
    assert!(matches!(
        Api::pair(server.url(), "ABCD1234"),
        Err(crate::Error::Api(ApiError::Status { status: 429, .. }))
    ));
    let mut original = Api::new(server.url(), server.token(), "DEMO0000001")?;
    original.session(&telemetry())?;
    Ok(())
}
