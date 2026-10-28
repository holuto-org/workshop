use std::{
    error::Error as StdError,
    io::{Read, Write},
    net::TcpStream,
    panic::catch_unwind,
};

use serde_json::{Value, json};
use uuid::Uuid;

use super::{Holuto, JOB_ID, job_json, telemetry, with};
use crate::api::{Api, ApiError};

type TestResult = Result<(), Box<dyn StdError>>;
const OTHER_PRINTER: Uuid = Uuid::from_u128(15);

fn api(server: &Holuto) -> Result<Api, Box<dyn StdError>> {
    let mut api = Api::new(server.url(), server.token(), "DEM0000000001")?;
    api.session(&telemetry())?;
    Ok(api)
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
        let mut job = job_json();
        let _etag = job.as_object_mut().unwrap().remove("etag");
        server.insert(job);
        let api = api(&server).unwrap();
        let _response = api.job(JOB_ID);
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
