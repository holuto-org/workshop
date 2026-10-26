use std::{
    error::Error as StdError,
    io::{Read, Write},
    net::TcpStream,
};

use serde_json::{Value, json};
use uuid::Uuid;

use super::{ALLOCATION_ID, Holuto, JOB_ID, ORDER_ID, PRINTER_ID, job_json, telemetry, with};
use crate::{
    api::{Api, ApiError, model::Job, queue_form},
    documents::Document,
};

type TestResult = Result<(), Box<dyn StdError>>;
const OTHER_ALLOCATION: Uuid = Uuid::from_u128(13);
const OTHER_ORDER: Uuid = Uuid::from_u128(14);
const OTHER_PRINTER: Uuid = Uuid::from_u128(15);
const STATES: [&str; 6] = ["queued", "taken", "done", "failed", "expired", "unknown"];

fn api(server: &Holuto) -> Result<Api, Box<dyn StdError>> {
    let mut api = Api::new(server.url(), server.token(), "DEM0000000001")?;
    api.session(&telemetry())?;
    Ok(api)
}

fn document(order: Uuid, allocation: Option<Uuid>, occurred_at: &str) -> Value {
    json!({
        "kind": if allocation.is_some() { "advance" } else { "sale" },
        "orderId": order, "orderNumber": 10, "allocationId": allocation,
        "amount": "250.00", "customerName": null, "occurredAt": occurred_at,
        "dueAt": occurred_at,
        "latestAt": occurred_at,
        "isOverdue": false
    })
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup and HTTP failures use Result"
)]
fn queue_refuses_unsupported_capabilities_and_quantity_precision_without_creating_work()
-> TestResult {
    for (advances, zero, precision, changes, expected_detail) in [
        (
            false,
            true,
            8,
            json!({
                "advances": [{"vatLetter": "A", "vatRate": "23", "amount": "100.00"}],
                "payments": [{"form": "card", "amount": "150.00"}]
            }),
            "Wybierz drukarkę obsługującą odliczanie zafiskalizowanych zaliczek \
             przed zleceniem paragonu końcowego.",
        ),
        (
            true,
            false,
            8,
            json!({
                "advances": [
                    {"vatLetter": "A", "vatRate": "23", "amount": "200.00"},
                    {"vatLetter": "B", "vatRate": "8", "amount": "50.00"}
                ], "payments": []
            }),
            "Wybierz drukarkę obsługującą zamknięcie w pełni opłaconego paragonu końcowego \
             z kwotą zerową.",
        ),
        (
            true,
            true,
            3,
            json!({
                "lines": [{"name": "Usługa", "quantity": "1.0001", "unitPrice": "250.00",
                    "total": "250.03", "vatRate": "23", "vatLetter": "A"}],
                "total": "250.03", "payments": [{"form": "card", "amount": "250.03"}]
            }),
            "Zmniejsz liczbę cyfr po przecinku w ilościach pozycji do limitu tej drukarki \
             przed zleceniem paragonu.",
        ),
    ] {
        let server = Holuto::new();
        server.queue_job(with(job_json(), &changes));
        let mut telemetry = telemetry();
        telemetry.capabilities.advance_deductions = advances;
        telemetry.capabilities.zero_total_receipts = zero;
        telemetry.capabilities.max_quantity_decimals = precision;
        let mut api = Api::new(server.url(), server.token(), "DEM0000000001")?;
        api.session(&telemetry)?;
        let document: Document =
            serde_json::from_value(document(ORDER_ID, None, "2026-10-16T10:00:00Z"))?;
        let result = api.queue(&queue_form(&document, None)?);
        assert!(
            matches!(&result,
                Err(ApiError::Status { status: 409, detail: Some(detail) }) if detail == expected_detail),
            "{result:?}"
        );
        assert!(matches!(api.job(Uuid::from_u128(100)),
                Err(ApiError::Status { status: 404, detail: Some(detail) })
                    if detail == "Nie znaleziono rekordu fiskalnego."));
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
                    "total": "112.30", "vatRate": "23", "vatLetter": "A"}],
                "total": "112.30", "payments": [{"form": "card", "amount": "112.30"}]
            }),
        ));
        let mut telemetry = telemetry();
        telemetry.capabilities.max_quantity_decimals = 3;
        let mut api = Api::new(server.url(), server.token(), "DEM0000000001")?;
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
                            if detail == "To zlecenie lub drukarka ma wystawiony paragon albo \
                                nierozstrzygniętą operację fiskalną. Przed kontynuowaniem \
                                sprawdź papierowy paragon."
                    ),
                    "{name}/{state}: expected exact fiscal_job_conflict, got {result:?}"
                );
                assert!(
                    matches!(
                        &api.job(Uuid::from_u128(101)),
                        Err(ApiError::Status { status: 404, detail: Some(detail) })
                            if detail == "Nie znaleziono rekordu fiskalnego."
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
            "other order sale before advance",
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
    // Hidden documents still anchor cursors.
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
    let mut original = Api::new(server.url(), server.token(), "DEM0000000001")?;
    assert!(matches!(
        original.session(&telemetry()),
        Err(ApiError::Unauthorized)
    ));
    let mut paired = Api::new(server.url(), first.token.as_str(), "DEM0000000001")?;
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
fn invalid_and_throttled_pairing_proofs_do_not_revoke_the_token() -> TestResult {
    let server = Holuto::new();
    server.pairing_code("ABCD1234");
    assert!(matches!(
        Api::pair(server.url(), "ZZZZZZZZ"),
        Err(crate::Error::Api(ApiError::Status {
            status: 400, detail: Some(detail),
        })) if detail == "fiscal_pairing_invalid"
    ));
    server.pairing_code("EFGH5678");
    server.pairing_throttled(true);
    assert!(matches!(
        Api::pair(server.url(), "EFGH5678"),
        Err(crate::Error::Api(ApiError::Status {
            status: 429, detail: Some(detail),
        })) if detail == "rate_limited"
    ));
    let mut original = Api::new(server.url(), server.token(), "DEM0000000001")?;
    original.session(&telemetry())?;
    server.pairing_throttled(false);
    assert_eq!(
        Api::pair(server.url(), "EFGH5678")?.token.as_str(),
        format!("{:0>42}A", 1)
    );
    Ok(())
}

fn raw(
    server: &Holuto,
    method: &str,
    path: &str,
    number: Option<&[u8]>,
    token: Option<&str>,
    body: &Value,
) -> Result<(u16, String, Value), Box<dyn StdError>> {
    let mut stream = TcpStream::connect(server.url().trim_start_matches("http://"))?;
    let body = serde_json::to_vec(body)?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    )?;
    if let Some(number) = number {
        stream.write_all(b"X-Device-Unique-Number: ")?;
        stream.write_all(number)?;
        stream.write_all(b"\r\n")?;
    }
    if let Some(token) = token {
        write!(stream, "Authorization: Bearer {token}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(&body)?;
    let mut response = String::new();
    let _read = stream.read_to_string(&mut response)?;
    let (headers, body) = response.split_once("\r\n\r\n").ok_or("missing HTTP body")?;
    let status = headers
        .split_whitespace()
        .nth(1)
        .ok_or("missing status")?
        .parse()?;
    Ok((status, headers.to_owned(), serde_json::from_str(body)?))
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_problems_have_localized_detail_and_problem_content_type() -> TestResult {
    let server = Holuto::new();
    let _api = api(&server)?;
    let (status, headers, body) = raw(
        &server,
        "GET",
        &format!("/agent/jobs/{}", Uuid::from_u128(999)),
        Some(b"DEM0000000001"),
        Some(server.token()),
        &Value::Null,
    )?;
    assert_eq!(status, 404);
    assert_eq!(body["code"], "fiscal_not_found");
    assert_eq!(body["detail"], "Nie znaleziono rekordu fiskalnego.");
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-type: application/problem+json")
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_unique_number_header_checks_precede_binding_and_token_checks() -> TestResult {
    let server = Holuto::new();
    for (number, token, status, code) in [
        (None, None, 400, "fiscal_invalid_input"),
        (Some(b"bad".as_slice()), None, 401, "unauthorized"),
        (
            Some(b"bad".as_slice()),
            Some(server.token()),
            400,
            "validation_failed",
        ),
        (
            Some(b"DEM0000000001".as_slice()),
            Some(server.token()),
            403,
            "fiscal_unique_number_mismatch",
        ),
    ] {
        let (actual, _, body) = raw(
            &server,
            "POST",
            "/agent/heartbeat",
            number,
            token,
            &Value::Null,
        )?;
        assert_eq!(actual, status, "{body}");
        assert_eq!(body["code"], code);
    }
    let _api = api(&server)?;
    let (status, _, body) = raw(
        &server,
        "POST",
        "/agent/heartbeat",
        Some(b"OTH0000000001"),
        Some(server.token()),
        &Value::Null,
    )?;
    assert_eq!(status, 403);
    assert_eq!(body["code"], "fiscal_unique_number_mismatch");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_session_rejects_duplicate_and_illegal_vat_letters_without_binding() -> TestResult {
    for (rates, code) in [
        (
            json!([{"letter":"A","vatRate":"23"},{"letter":"A","vatRate":"23"}]),
            "fiscal_printer_duplicate_vat_letter",
        ),
        (
            json!([{"letter":"A","vatRate":"8"}]),
            "fiscal_printer_vat_letter_illegal",
        ),
        (
            json!([{"letter":"B","vatRate":"5"}]),
            "fiscal_printer_vat_letter_illegal",
        ),
        (
            json!([{"letter":"C","vatRate":"0"}]),
            "fiscal_printer_vat_letter_illegal",
        ),
        (
            json!([{"letter":"D","vatRate":"zw"}]),
            "fiscal_printer_vat_letter_illegal",
        ),
        (
            json!([{"letter":"E","vatRate":"23"}]),
            "fiscal_printer_vat_letter_illegal",
        ),
    ] {
        let server = Holuto::new();
        let mut body = serde_json::to_value(telemetry())?;
        body["vatRates"] = rates;
        let (status, _, problem) = raw(
            &server,
            "POST",
            "/agent/session",
            None,
            Some(server.token()),
            &body,
        )?;
        assert_eq!(status, 400, "{problem}");
        assert_eq!(problem["code"], "validation_failed");
        assert_eq!(problem["errors"][0]["code"], code);
        assert!(super::locked(&server.state).unique_number.is_none());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_queue_rejects_allocation_kind_mismatch_without_creating_work() -> TestResult {
    for (kind, allocation) in [("advance", Value::Null), ("sale", json!(ALLOCATION_ID))] {
        let server = Holuto::new();
        let api = api(&server)?;
        let (status, _, body) = raw(
            &server,
            "POST",
            "/agent/jobs",
            Some(b"DEM0000000001"),
            Some(server.token()),
            &json!({"orderId":ORDER_ID,"kind":kind,"allocationId":allocation,"buyerNip":null}),
        )?;
        assert_eq!(status, 400, "{body}");
        assert_eq!(body["code"], "validation_failed");
        assert_eq!(body["errors"][0]["code"], "fiscal_receipt_allocation_kind");
        assert!(api.take()?.is_none());
        assert!(super::locked(&server.state).jobs.is_empty());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_documents_validate_limit_and_default_to_fifty() -> TestResult {
    let server = Holuto::new();
    let _api = api(&server)?;
    server.documents(
        (1..=101)
            .map(|id| document(Uuid::from_u128(id), None, "2026-10-16T10:00:00Z"))
            .collect(),
    );
    for limit in ["0", "101", "-1", "1.5", "no", ""] {
        let (status, ..) = raw(
            &server,
            "GET",
            &format!("/agent/documents?limit={limit}"),
            Some(b"DEM0000000001"),
            Some(server.token()),
            &Value::Null,
        )?;
        assert_eq!(status, 400, "limit={limit}");
    }
    for (query, count) in [("", 50), ("?limit=1", 1), ("?limit=100", 100)] {
        let (status, _, body) = raw(
            &server,
            "GET",
            &format!("/agent/documents{query}"),
            Some(b"DEM0000000001"),
            Some(server.token()),
            &Value::Null,
        )?;
        assert_eq!(status, 200);
        let expected: Vec<_> = (1..=count)
            .map(|id| document(Uuid::from_u128(id), None, "2026-10-16T10:00:00Z"))
            .collect();
        assert_eq!(
            body,
            json!({
                "items":expected, "nextCursor":Uuid::from_u128(count).to_string()
            })
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_result_normalizes_code_and_compares_replays_at_microsecond_precision() -> TestResult {
    let server = Holuto::new();
    server.insert(job_json());
    let _api = api(&server)?;
    let path = format!("/agent/jobs/{JOB_ID}/result");
    let mut form = json!({
        "attempt":super::ATTEMPT, "state":"done", "receiptNumber":"  ＡＢ  １２３\u{2003} ４ ",
        "issuedAt":"2025-01-02T12:00:00.123456789Z", "result":null
    });
    let (status, _, body) = raw(
        &server,
        "POST",
        &path,
        Some(b"DEM0000000001"),
        Some(server.token()),
        &form,
    )?;
    assert_eq!(status, 200);
    assert_eq!(body["receiptNumber"], "AB 123 4");
    assert_eq!(server.job(JOB_ID)["receiptNumber"], "AB 123 4");
    form["receiptNumber"] = json!("AB 123 4");
    form["issuedAt"] = json!("2025-01-02T12:00:00.123456001Z");
    assert_eq!(
        raw(
            &server,
            "POST",
            &path,
            Some(b"DEM0000000001"),
            Some(server.token()),
            &form
        )?
        .0,
        200
    );
    form["issuedAt"] = json!("2025-01-02T12:00:00.123457Z");
    assert_eq!(
        raw(
            &server,
            "POST",
            &path,
            Some(b"DEM0000000001"),
            Some(server.token()),
            &form
        )?
        .0,
        409
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_result_refuses_text_it_cannot_store_without_committing() -> TestResult {
    for (state, field, value, code) in [
        (
            "done",
            "receiptNumber",
            "12\u{200b}34",
            "invalid_characters",
        ),
        (
            "done",
            "receiptNumber",
            "12\u{2028}34",
            "invalid_characters",
        ),
        ("done", "receiptNumber", " \u{3000} ", "blank"),
        ("failed", "result", "Błąd\u{200b}", "invalid_characters"),
        ("failed", "result", " \u{3000} ", "blank"),
    ] {
        let server = Holuto::new();
        server.insert(job_json());
        let _api = api(&server)?;
        let done = state == "done";
        let mut form = json!({
            "attempt": super::ATTEMPT, "state": state, "result": null, "receiptNumber": null,
            "issuedAt": if done { json!("2025-01-02T12:00:00Z") } else { Value::Null },
        });
        form[field] = json!(value);
        let (status, _, body) = raw(
            &server,
            "POST",
            &format!("/agent/jobs/{JOB_ID}/result"),
            Some(b"DEM0000000001"),
            Some(server.token()),
            &form,
        )?;
        assert_eq!(
            (
                status,
                &body["errors"][0]["pointer"],
                &body["errors"][0]["code"]
            ),
            (400, &json!(format!("/{field}")), &json!(code)),
            "{field} {value:?}"
        );
        assert_eq!(server.job(JOB_ID)["state"], "taken", "{field} {value:?}");
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_jobs_of_another_printer_are_invisible_and_never_block_take() -> TestResult {
    let server = Holuto::new();
    server.insert(with(job_json(), &json!({"printerId": OTHER_PRINTER})));
    let own = server.enqueue(with(job_json(), &json!({"id": Uuid::from_u128(200)})));
    let api = api(&server)?;
    assert!(matches!(
        api.job(JOB_ID),
        Err(ApiError::Status { status: 404, .. })
    ));
    assert_eq!(api.take()?.map(|job| job.id), Some(own));
    Ok(())
}
