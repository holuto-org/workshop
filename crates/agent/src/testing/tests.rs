use std::{
    error::Error as StdError,
    io::{Read, Write},
    net::TcpStream,
    panic::catch_unwind,
};

use serde_json::{Value, json};
use uuid::Uuid;

use super::{ALLOCATION_ID, Holuto, JOB_ID, ORDER_ID, PRINTER_ID, job_json, telemetry, with};
use crate::{
    api::{Api, ApiError, model::Job, queue_form},
    documents::{Document, DocumentSearch},
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
fn fake_request_contract_violations_fail_the_owning_test() {
    let outcome = catch_unwind(|| {
        let server = Holuto::new();
        let mut api = Api::new(server.url(), server.token(), "DEM0000000001").unwrap();
        let mut invalid = telemetry();
        invalid.model.clear();
        let _response = api.session(&invalid);
        drop(server);
    });
    assert!(
        outcome.is_err(),
        "the HTTP fake must propagate its contract panic"
    );
}

#[test]
fn fake_response_contract_violations_fail_the_owning_test() {
    let outcome = catch_unwind(|| {
        let server = Holuto::new();
        server.page(None, json!({"items": [], "nextCursor": null}));
        let api = api(&server).unwrap();
        let _response = api.documents(None, None);
        drop(server);
    });
    assert!(
        outcome.is_err(),
        "the HTTP fake must propagate its contract panic"
    );
}

#[test]
fn fake_contract_panic_during_test_unwind_does_not_abort_the_process() {
    let outcome = catch_unwind(|| {
        let server = Holuto::new();
        let mut api = Api::new(server.url(), server.token(), "DEM0000000001").unwrap();
        let mut invalid = telemetry();
        invalid.model.clear();
        let _response = api.session(&invalid);
        panic!("owning test is already unwinding");
    });
    assert_eq!(
        outcome.unwrap_err().downcast_ref::<&str>(),
        Some(&"owning test is already unwinding")
    );
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
            matches!(
                &result,
                Err(ApiError::Status { status: 409, detail: Some(detail) })
                    if detail == expected_detail
            ),
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
            let page = api(&server)?.documents(None, None)?;
            let expected = if hidden { json!([]) } else { json!([value]) };
            assert_eq!(
                serde_json::to_value(page)?,
                json!({"items": expected, "nextCursor": null, "previousCursor": null}),
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
    let first = api.documents(None, None)?;
    assert_eq!(serde_json::to_value(&first.items)?, json!(first_items));
    let cursor = first.next_cursor.ok_or("missing continuation")?;

    api.queue(&queue_form(&serde_json::from_value(first_advance)?, None)?)?;

    let continuation = api.documents(Some(&cursor), None)?;
    assert_eq!(
        serde_json::to_value(&continuation.items)?,
        json!([next_advance, next_sale])
    );
    assert!(continuation.next_cursor.is_none());
    let previous = continuation
        .previous_cursor
        .ok_or("missing previous cursor")?;
    let _removed = first_items.pop();
    let back = api.documents(Some(&previous), None)?;
    assert_eq!(serde_json::to_value(back.items)?, json!(first_items));
    assert!(back.previous_cursor.is_none());
    first_items.push(next_advance);
    let refreshed = api.documents(None, None)?;
    assert_eq!(serde_json::to_value(refreshed.items)?, json!(first_items));
    assert!(refreshed.next_cursor.is_some());
    assert!(refreshed.previous_cursor.is_none());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup and HTTP failures use Result"
)]
fn document_search_requires_every_word_and_pages_within_the_matches() -> TestResult {
    let server = Holuto::new();
    let named = |id: u128, number: i64, customer: &str| {
        let mut value = document(Uuid::from_u128(id), None, "2025-10-16T10:00:00Z");
        value["orderNumber"] = json!(number);
        value["customerName"] = json!(customer);
        value
    };
    let mut fixtures: Vec<_> = (1..=101_u8)
        .map(|number| named(u128::from(number) + 5000, i64::from(number), "Jan Kowalski"))
        .collect();
    fixtures.extend([
        named(4000, 500, "Anna Kowalska"),
        named(5050, 600, "Janusz Nowak"),
        named(9000, 700, "Jan Kowalski"),
    ]);
    // The last fixture is queued away: search never reveals an ineligible document.
    server.documents(fixtures.clone());
    let api = api(&server)?;
    let queued: Document = serde_json::from_value(fixtures.pop().ok_or("missing fixture")?)?;
    api.queue(&queue_form(&queued, None)?)?;
    let search = DocumentSearch::parse("KOWAL, jan")?;
    let first = api.documents(None, search.as_ref())?;
    let numbers = |items: &[Document]| -> Vec<i64> {
        items.iter().map(|document| document.order_number).collect()
    };
    assert_eq!(numbers(&first.items), (1..=100).collect::<Vec<_>>());
    let cursor = first.next_cursor.ok_or("missing continuation")?;
    let rest = api.documents(Some(&cursor), search.as_ref())?;
    assert_eq!(
        (numbers(&rest.items), rest.next_cursor),
        (vec![101], None),
        "the cursor continues the same search"
    );
    let by_number = api.documents(None, DocumentSearch::parse("600")?.as_ref())?;
    assert_eq!(numbers(&by_number.items), [600]);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "empty live windows must retain a path back through the production client"
)]
fn empty_document_windows_keep_the_opposite_direction_including_the_boundary() -> TestResult {
    let server = Holuto::new();
    let fixtures: Vec<_> = (1..=101)
        .map(|id| document(Uuid::from_u128(id), None, "2026-10-16T10:00:00Z"))
        .collect();
    server.documents(fixtures);
    let api = api(&server)?;
    let first = api.documents(None, None)?;
    let forward = first.next_cursor.ok_or("missing forward cursor")?;
    let last = api.documents(Some(&forward), None)?;
    let backward = last.previous_cursor.ok_or("missing backward cursor")?;
    let last_document = last.items.first().ok_or("missing last document")?;
    api.queue(&queue_form(last_document, None)?)?;
    let empty = api.documents(Some(&forward), None)?;
    assert_eq!(empty.items, []);
    assert!(empty.next_cursor.is_none());
    let back = api.documents(
        Some(
            &empty
                .previous_cursor
                .ok_or("missing reverse path from empty window")?,
        ),
        None,
    )?;
    assert_eq!(back.items.len(), 100);
    assert_eq!(
        back.items.first().ok_or("missing first document")?.order_id,
        Uuid::from_u128(1)
    );
    assert_eq!(
        back.items
            .last()
            .ok_or("missing boundary document")?
            .order_id,
        Uuid::from_u128(100)
    );

    // Make the queued sale available again, but remove the entire earlier window.
    let taken = api.take()?.ok_or("missing queued job")?;
    server.set_job(taken.id, &json!({"state": "failed"}));
    server.documents(vec![document(
        Uuid::from_u128(101),
        None,
        "2026-10-16T10:00:00Z",
    )]);
    let empty = api.documents(Some(&backward), None)?;
    assert_eq!(empty.items, []);
    assert!(empty.previous_cursor.is_none());
    let next = api.documents(
        Some(
            &empty
                .next_cursor
                .ok_or("missing forward path from empty window")?,
        ),
        None,
    )?;
    assert_eq!(next.items.len(), 1);
    assert_eq!(next.items[0].order_id, Uuid::from_u128(101));
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
    assert_eq!(body["issuedAt"], "2025-01-02T12:00:00.123456Z");
    assert_eq!(
        server.job(JOB_ID)["issuedAt"],
        "2025-01-02T12:00:00.123456Z"
    );
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

/// `raw` cannot read the chunked bodies of full history windows.
fn jobs_search(server: &Holuto, body: &Value) -> Result<(u16, Value), Box<dyn StdError>> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .proxy(None)
            .build(),
    );
    let mut response = agent
        .post(format!("{}/agent/jobs/search", server.url()))
        .header("Authorization", format!("Bearer {}", server.token()))
        .header("X-Device-Unique-Number", "DEM0000000001")
        .send_json(body)?;
    let status = response.status().as_u16();
    let body = serde_json::from_reader(response.body_mut().as_reader())?;
    Ok((status, body))
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP assertions use Result for setup and inspect the returned nonempty window"
)]
fn backend_jobs_history_scopes_all_states_and_keeps_metadata_outside_the_job() -> TestResult {
    let server = Holuto::new();
    let _api = api(&server)?;
    for (index, state) in STATES.into_iter().chain(["voided"]).enumerate() {
        let issued_at = ["done", "voided"]
            .contains(&state)
            .then_some("2025-01-02T12:00:00Z");
        server.insert(with(
            job_json(),
            &json!({
                "id": Uuid::from_u128(100 + u128::try_from(index)?), "state": state,
                "issuedAt": issued_at
            }),
        ));
    }
    server.insert(with(
        job_json(),
        &json!({"id": Uuid::from_u128(999), "printerId": OTHER_PRINTER}),
    ));
    server.set_job_metadata(
        Uuid::from_u128(106),
        &json!({"customerName": "Łucja Żółć", "vehiclePlate": "WA12345", "source": "staff"}),
    );
    let (status, page) = jobs_search(
        &server,
        &json!({"filter": {"state": null, "orderId": null}, "search": {}, "cursor": {}}),
    )?;
    assert_eq!(status, 200);
    let items = page["items"].as_array().ok_or("missing jobs")?;
    let ids: Vec<_> = items.iter().map(|item| item["job"]["id"].clone()).collect();
    assert_eq!(
        ids,
        json!([
            Uuid::from_u128(106),
            Uuid::from_u128(105),
            Uuid::from_u128(104),
            Uuid::from_u128(103),
            Uuid::from_u128(102),
            Uuid::from_u128(101),
            Uuid::from_u128(100)
        ])
        .as_array()
        .ok_or("expected ids")?
        .clone()
    );
    assert_eq!(items[0]["customerName"], "Łucja Żółć");
    assert_eq!(items[0]["vehiclePlate"], "WA12345");
    assert_eq!(items[0]["job"]["etag"], "0123456789abcdef0123456789abcdef");
    assert_eq!(items[0]["job"]["state"], "voided");
    for item in items {
        assert!(item.get("issuedAt").is_none());
        assert!(item.get("customerName").is_some());
        assert!(item.get("vehiclePlate").is_some());
        assert_eq!(item["source"], "staff");
        assert!(item["job"].get("customerName").is_none());
        assert!(item["job"].get("issuedAt").is_some());
    }
    assert_eq!(page["nextCursor"], Value::Null);
    assert_eq!(page["previousCursor"], Value::Null);
    let (status, filtered) = jobs_search(
        &server,
        &json!({"filter": {"state": "expired", "orderId": ORDER_ID}, "search": {}, "cursor": {}}),
    )?;
    assert_eq!(status, 200);
    assert_eq!(filtered["items"].as_array().ok_or("missing jobs")?.len(), 1);
    assert_eq!(filtered["items"][0]["job"]["state"], "expired");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_jobs_search_folds_accents_and_requires_every_literal_word() -> TestResult {
    let server = Holuto::new();
    let _api = api(&server)?;
    server.insert(with(job_json(), &json!({"receiptNumber": "123/ABC"})));
    server.set_job_metadata(
        JOB_ID,
        &json!({"customerName": "Łucja Żółć", "vehiclePlate": "WA12345"}),
    );
    for (q, count) in [
        ("lucja", 1),
        ("zolc", 1),
        ("wa12345", 1),
        ("123/abc", 1),
        ("10", 1),
        ("OLEJU", 1),
        ("lucja,oleju", 1),
        ("lucja missing", 0),
        ("%", 0),
        ("_", 0),
    ] {
        let (status, page) = jobs_search(
            &server,
            &json!({"filter": {}, "search": {"q": q}, "cursor": {}}),
        )?;
        assert_eq!(status, 200, "{q}");
        assert_eq!(
            page["items"].as_array().ok_or("missing jobs")?.len(),
            count,
            "{q}"
        );
    }
    let debug = format!("{:?}", server.requests());
    assert!(!debug.contains("lucja"));
    assert!(!debug.contains("wa12345"));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_jobs_cursor_navigates_both_directions_and_binds_criteria() -> TestResult {
    let server = Holuto::new();
    let _api = api(&server)?;
    for id in [100, 101, 102] {
        server.insert(with(job_json(), &json!({"id": Uuid::from_u128(id)})));
    }
    let mut form = json!({
        "filter": {}, "search": {}, "sort": ["id"], "cursor": {"limit": 1}
    });
    let (status, first) = jobs_search(&server, &form)?;
    assert_eq!(status, 200);
    assert_eq!(first["items"][0]["job"]["id"], json!(Uuid::from_u128(100)));
    assert_eq!(first["previousCursor"], Value::Null);
    let token = first["nextCursor"].as_str().ok_or("missing next cursor")?;
    form["cursor"]["cursor"] = json!(token);
    let (status, second) = jobs_search(&server, &form)?;
    assert_eq!(status, 200);
    assert_eq!(second["items"][0]["job"]["id"], json!(Uuid::from_u128(101)));
    form["cursor"]["cursor"] = second["previousCursor"].clone();
    let (status, previous) = jobs_search(&server, &form)?;
    assert_eq!(status, 200);
    assert_eq!(
        previous["items"][0]["job"]["id"],
        json!(Uuid::from_u128(100))
    );
    form["cursor"]["cursor"] = json!(token);
    for changes in [
        json!({"filter": {"state": "done"}}),
        json!({"filter": {"orderId": OTHER_ORDER}}),
        json!({"search": {"q": "oleju"}}),
        json!({"sort": ["-id"]}),
    ] {
        let changed = with(form.clone(), &changes);
        let (status, body) = jobs_search(&server, &changed)?;
        assert_eq!(status, 400);
        assert_eq!(body["code"], "fiscal_invalid_input");
        assert_eq!(body["errors"][0]["pointer"], "/cursor/cursor");
        assert_eq!(body["errors"][0]["code"], "invalid_cursor");
    }
    form["cursor"]["cursor"] = json!("not-a-cursor");
    assert_eq!(jobs_search(&server, &form)?.0, 400);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_jobs_search_refuses_invalid_criteria_without_mutating_jobs() -> TestResult {
    let server = Holuto::new();
    let _api = api(&server)?;
    server.insert(job_json());
    for (changes, pointer, code) in [
        (json!({"search": {"q": ""}}), "/search/q", "blank"),
        (json!({"search": {"q": " \u{3000} "}}), "/search/q", "blank"),
        (
            json!({"search": {"q": "x".repeat(129)}}),
            "/search/q",
            "too_long",
        ),
        (
            json!({"cursor": {"limit": 0}}),
            "/cursor/limit",
            "invalid_page_size",
        ),
        (
            json!({"cursor": {"limit": 101}}),
            "/cursor/limit",
            "invalid_page_size",
        ),
        (
            json!({"cursor": {"limit": -1}}),
            "/cursor/limit",
            "invalid_page_size",
        ),
        (
            json!({"cursor": {"limit": 1.5}}),
            "/cursor/limit",
            "invalid_page_size",
        ),
        (
            json!({"cursor": {"limit": "no"}}),
            "/cursor/limit",
            "invalid_page_size",
        ),
        (
            json!({"filter": {"printerId": PRINTER_ID}}),
            "/filter/printerId",
            "unknown_field",
        ),
        (json!({"cursor": null}), "/cursor", "required"),
    ] {
        let form = with(json!({"filter": {}, "search": {}, "cursor": {}}), &changes);
        let (status, body) = jobs_search(&server, &form)?;
        assert_eq!(status, 400);
        assert_eq!(body["code"], "fiscal_invalid_input");
        assert_eq!(body["errors"][0]["pointer"], pointer);
        assert_eq!(body["errors"][0]["code"], code);
        assert_eq!(server.job(JOB_ID), job_json());
    }
    for q in ["x".to_owned(), "x".repeat(128)] {
        assert_eq!(
            jobs_search(
                &server,
                &json!({"filter": {}, "search": {"q": q}, "cursor": {"limit": 100}}),
            )?
            .0,
            200
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_jobs_history_exposes_report_issue_time_and_device_source() -> TestResult {
    let server = Holuto::new();
    let api = api(&server)?;
    let document: Document =
        serde_json::from_value(document(ORDER_ID, None, "2026-10-16T10:00:00Z"))?;
    api.queue(&queue_form(&document, None)?)?;
    let taken = api.take()?.ok_or("missing job")?;
    assert_eq!(taken.issued_at, None);
    let form = json!({
        "attempt": taken.attempt, "state": "done", "receiptNumber": "456",
        "issuedAt": "2025-01-02T12:00:00Z", "result": null
    });
    assert_eq!(
        raw(
            &server,
            "POST",
            &format!("/agent/jobs/{}/result", taken.id),
            Some(b"DEM0000000001"),
            Some(server.token()),
            &form,
        )?
        .0,
        200
    );
    let (status, page) = jobs_search(&server, &json!({"filter": {}, "search": {}, "cursor": {}}))?;
    assert_eq!(status, 200);
    assert_eq!(page["items"][0]["job"]["issuedAt"], "2025-01-02T12:00:00Z");
    assert_eq!(page["items"][0]["source"], "device");
    assert_eq!(page["items"][0]["job"]["receiptNumber"], "456");
    assert!(page["items"][0].get("issuedAt").is_none());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup"
)]
fn backend_jobs_sorting_and_default_window_use_created_at_then_id() -> TestResult {
    let server = Holuto::new();
    let _api = api(&server)?;
    for id in 100..=200 {
        server.insert(with(job_json(), &json!({"id": Uuid::from_u128(id)})));
    }
    // Timestamp comparison must compare instants, not their textual UTC offsets.
    server.set_job(
        Uuid::from_u128(100),
        &json!({"createdAt": "2026-10-16T12:00:00+01:00"}),
    );
    let (_, page) = jobs_search(&server, &json!({"filter": {}, "search": {}, "cursor": {}}))?;
    assert_eq!(page["items"].as_array().ok_or("missing jobs")?.len(), 50);
    assert_eq!(page["items"][0]["job"]["id"], json!(Uuid::from_u128(100)));
    assert_eq!(page["items"][1]["job"]["id"], json!(Uuid::from_u128(200)));
    assert!(page["nextCursor"].is_string());
    for (sort, expected) in [
        (json!(["createdAt", "id"]), 101),
        (json!(["-createdAt", "id"]), 100),
        (json!(["-id"]), 200),
    ] {
        let (status, page) = jobs_search(
            &server,
            &json!({"filter": {}, "search": {}, "sort": sort, "cursor": {"limit": 1}}),
        )?;
        assert_eq!(status, 200);
        assert_eq!(
            page["items"][0]["job"]["id"],
            json!(Uuid::from_u128(expected))
        );
    }
    let (_, page) = jobs_search(
        &server,
        &json!({"filter": {"orderId": OTHER_ORDER}, "search": {}, "cursor": {"limit": 100}}),
    )?;
    assert_eq!(
        page,
        json!({"items": [], "nextCursor": null, "previousCursor": null})
    );
    Ok(())
}
