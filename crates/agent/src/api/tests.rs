use std::{env, error::Error as StdError, process::Command, sync::Arc};

use sentry_core::{
    Client, ClientOptions, Envelope, Hub, Scope, TransactionContext, Transport, start_transaction,
};
use serde_json::json;
use uuid::Uuid;

use super::{
    Api, ApiError,
    model::{Outcome, ResultForm, Telemetry},
};
use crate::{
    Error,
    testing::{
        Fault, Holuto, JOB_ID, PRINTER_ID, job_json, status_json, telemetry as telemetry_for, with,
    },
};

type TestResult = Result<(), Box<dyn StdError>>;
fn telemetry() -> Telemetry {
    telemetry_for("DEMO0000001")
}
fn api(server: &Holuto) -> Result<Api, Error> {
    Api::new(server.url(), server.token(), "DEMO0000001")
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn results_are_idempotent_after_the_server_committed_but_lost_the_reply() -> TestResult {
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let mut api = api(&server)?;
    api.session(&telemetry())?;
    let job = api.take()?.ok_or("missing claimed job")?;
    let form = ResultForm {
        attempt: job.attempt.ok_or("no attempt")?,
        issued_at: Some("2025-01-02T12:00:00Z".parse()?),
        receipt_number: Some("123".to_owned()),
        result: None,
        state: Outcome::Done,
    };
    server.fault(
        "POST",
        &format!("/agent/jobs/{JOB_ID}/result"),
        Fault::DropAfterCommit,
    );
    assert!(
        api.report(job.id, &form).is_err(),
        "lost reply must be observable"
    );
    api.report(job.id, &form)?;
    assert_eq!(server.job(job.id)["state"], "done");
    assert_eq!(server.job(job.id)["receiptNumber"], "123");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn backend_normalization_of_result_text_does_not_wedge_settlement() -> TestResult {
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let mut api = api(&server)?;
    api.session(&telemetry())?;
    let job = api.take()?.ok_or("missing job")?;
    let form = ResultForm {
        attempt: job.attempt.ok_or("no attempt")?,
        issued_at: None,
        receipt_number: None,
        result: Some("  brak papieru  ".to_owned()),
        state: Outcome::Failed,
    };
    api.report(job.id, &form)?;
    assert_eq!(server.job(job.id)["result"], "brak papieru");
    api.report(job.id, &form)?;
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn oversized_and_garbled_responses_are_contract_errors_not_connection_errors() -> TestResult {
    let server = Holuto::new();
    let mut api = api(&server)?;
    for fault in [
        Fault::Oversized,
        Fault::Body {
            status: 200,
            body: b"not json".to_vec(),
        },
    ] {
        server.fault("POST", "/agent/session", fault);
        assert!(
            matches!(api.session(&telemetry()), Err(ApiError::Response(_))),
            "invalid server data must be diagnosed as invalid response"
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn future_printer_issue_time_is_rejected_without_committing_a_result() -> TestResult {
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let mut api = api(&server)?;
    api.session(&telemetry())?;
    let job = api.take()?.ok_or("missing job")?;
    let form = ResultForm {
        attempt: job.attempt.ok_or("no attempt")?,
        issued_at: Some("2099-01-02T12:00:00Z".parse()?),
        receipt_number: Some("123".to_owned()),
        result: None,
        state: Outcome::Done,
    };
    assert!(
        matches!(
            api.report(job.id, &form),
            Err(ApiError::Status { status: 400, .. })
        ),
        "backend refuses future fiscal evidence"
    );
    assert_eq!(server.job(job.id)["state"], "taken");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn tokens_are_redacted_from_api_debug_and_echoed_server_errors() -> TestResult {
    let server = Holuto::new();
    let mut api = api(&server)?;
    assert!(
        !format!("{api:?}").contains(server.token()),
        "Api debug must never expose authorization"
    );
    server.fault(
        "POST",
        "/agent/session",
        Fault::Problem {
            status: 409,
            code: "conflict".to_owned(),
            detail: format!("bad {}", server.token()),
        },
    );
    let error = api.session(&telemetry()).err().ok_or("missing rejection")?;
    assert!(
        !format!("{error:?} {error}").contains(server.token()),
        "even an upstream token echo is redacted"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn every_endpoint_uses_device_credentials_and_correct_serial_binding() -> TestResult {
    let server = Holuto::new();
    let _id = server.enqueue(job_json());
    let mut api = api(&server)?;
    api.session(&telemetry())?;
    api.heartbeat(&telemetry())?;
    let _page = api.documents(None)?;
    let job = api.take()?.ok_or("missing job")?;
    let _remote = api.job(job.id)?;
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/agent/session",
            "/agent/heartbeat",
            "/agent/documents?limit=100",
            "/agent/jobs/take",
            "/agent/jobs/00000000-0000-0000-0000-000000000007"
        ]
    );
    for (index, request) in requests.iter().enumerate() {
        assert!(
            request.authenticated,
            "all requests must bear device credential"
        );
        assert_eq!(request.language.as_deref(), Some("pl"));
        assert_eq!(
            request.app_version.as_deref(),
            Some(concat!("agent/", env!("CARGO_PKG_VERSION")))
        );
        assert_eq!(
            request.serial.as_deref(),
            if index < 2 { None } else { Some("DEMO0000001") }
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn unauthorized_stops_without_refresh_or_retry() -> TestResult {
    let server = Holuto::new();
    server.unauthorized(true);
    let mut api = api(&server)?;
    assert!(
        matches!(api.session(&telemetry()), Err(ApiError::Unauthorized)),
        "401 has a distinct stop-processing error"
    );
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/agent/session"]
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn unauthorized_status_stops_even_when_the_error_body_exceeds_the_limit() -> TestResult {
    let server = Holuto::new();
    server.fault(
        "POST",
        "/agent/session",
        Fault::Body {
            status: 401,
            body: vec![b'x'; 4 * 1024 * 1024 + 1],
        },
    );
    let mut api = api(&server)?;
    assert!(
        matches!(api.session(&telemetry()), Err(ApiError::Unauthorized)),
        "body decoding must never mask a definitive token rejection"
    );
    assert_eq!(server.requests().len(), 1, "401 causes no refresh or retry");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn redirects_never_forward_device_credentials() -> TestResult {
    let server = Holuto::new();
    server.fault(
        "POST",
        "/agent/session",
        Fault::Redirect("/steal-credentials".to_owned()),
    );
    let mut api = api(&server)?;
    assert!(
        matches!(
            api.session(&telemetry()),
            Err(ApiError::Status { status: 302, .. })
        ),
        "redirects are refused"
    );
    assert_eq!(
        server.requests().len(),
        1,
        "redirect target must receive no request"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn non_json_server_error_retains_status_not_a_fake_network_diagnosis() -> TestResult {
    let server = Holuto::new();
    server.fault(
        "POST",
        "/agent/session",
        Fault::Body {
            status: 503,
            body: b"upstream unavailable".to_vec(),
        },
    );
    let mut api = api(&server)?;
    assert!(
        matches!(
            api.session(&telemetry()),
            Err(ApiError::Status {
                status: 503,
                detail: None
            })
        ),
        "HTTP errors remain HTTP errors even without problem JSON"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn malformed_nullable_session_fields_and_foreign_serial_are_rejected() -> TestResult {
    for changes in [
        json!({"deviceSerial":"OTHER"}),
        json!({"maxItemNameLength":0}),
        json!({"allowedCharacters":""}),
    ] {
        let server = Holuto::new();
        let mut status = status_json();
        status["printer"] = with(status["printer"].clone(), &changes);
        server.fault(
            "POST",
            "/agent/session",
            Fault::Body {
                status: 200,
                body: serde_json::to_vec(&status)?,
            },
        );
        let mut api = api(&server)?;
        assert!(
            matches!(api.session(&telemetry()), Err(ApiError::Response(_))),
            "foreign or invalid printer metadata cannot bind the session"
        );
    }
    let server = Holuto::new();
    let mut status = status_json();
    let _removed = status["printer"]
        .as_object_mut()
        .ok_or("not object")?
        .remove("deviceSerial");
    server.fault(
        "POST",
        "/agent/session",
        Fault::Body {
            status: 200,
            body: serde_json::to_vec(&status)?,
        },
    );
    let mut api = api(&server)?;
    assert!(
        matches!(api.session(&telemetry()), Err(ApiError::Response(_))),
        "required nullable fields cannot disappear"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn mismatched_job_id_and_result_attempt_are_not_acknowledged() -> TestResult {
    let server = Holuto::new();
    server.insert(job_json());
    let mut api = api(&server)?;
    api.session(&telemetry())?;
    let foreign = with(job_json(), &json!({"id":Uuid::from_u128(88)}));
    server.fault(
        "GET",
        &format!("/agent/jobs/{JOB_ID}"),
        Fault::Body {
            status: 200,
            body: serde_json::to_vec(&foreign)?,
        },
    );
    assert!(
        matches!(api.job(JOB_ID), Err(ApiError::Response(_))),
        "status for another job is rejected"
    );
    let form = ResultForm {
        attempt: Uuid::from_u128(99),
        issued_at: None,
        receipt_number: None,
        result: Some("No dispatch".to_owned()),
        state: Outcome::Failed,
    };
    assert!(
        matches!(
            api.report(JOB_ID, &form),
            Err(ApiError::Status { status: 409, .. })
        ),
        "wrong attempt cannot overwrite result"
    );
    assert_eq!(server.job(JOB_ID)["state"], "taken");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn opaque_cursor_is_encoded_once_and_an_empty_cursor_is_never_sent() -> TestResult {
    let server = Holuto::new();
    let cursor = "a+/=?ąć%";
    server.page(Some(cursor), json!({"items":[],"nextCursor":null}));
    let mut api = api(&server)?;
    api.session(&telemetry())?;
    let _page = api.documents(Some(cursor))?;
    assert_eq!(
        server.requests().last().ok_or("no request")?.path,
        "/agent/documents?limit=100&cursor=a%2B%2F%3D%3F%C4%85%C4%87%25"
    );
    let count = server.requests().len();
    assert!(
        matches!(api.documents(Some("")), Err(ApiError::Response(_))),
        "empty cursor is a local contract error"
    );
    assert_eq!(server.requests().len(), count);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_malformed_page_is_rejected_whole() -> TestResult {
    let server = Holuto::new();
    server.fault(
        "GET",
        "/agent/documents",
        Fault::Body {
            status: 200,
            body: serde_json::to_vec(&json!({"items":[],"nextCursor":""}))?,
        },
    );
    let mut api = api(&server)?;
    api.session(&telemetry())?;
    assert!(
        matches!(api.documents(None), Err(ApiError::Response(_))),
        "bad continuation must not silently end pagination"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn invalid_physical_serial_is_refused_without_sending() -> TestResult {
    let server = Holuto::new();
    let _valid = api(&server)?;
    for serial in ["", " SERIAL", "SERIAL ", "ŁÓDŹ", "bad\nserial"] {
        assert!(
            matches!(
                Api::new(server.url(), server.token(), serial),
                Err(Error::Invalid(_))
            ),
            "serial identity is never canonicalized or truncated"
        );
    }
    assert!(
        server.requests().is_empty(),
        "invalid serial causes no request"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn loopback_api_never_sends_its_token_to_an_environment_proxy() -> TestResult {
    const CHILD: &str = "HOLUTO_AGENT_PROXY_CHILD";
    if env::var_os(CHILD).is_some() {
        let server = Holuto::new();
        let mut api = api(&server)?;
        api.session(&telemetry())?;
        assert!(
            server
                .requests()
                .first()
                .ok_or("no direct request")?
                .authenticated,
            "credential reaches the loopback peer directly"
        );
        return Ok(());
    }
    let proxy = Holuto::new();
    #[expect(
        clippy::disallowed_methods,
        reason = "proxy environment is isolated in a child test process"
    )]
    let status = Command::new(env::current_exe()?)
        .arg("--exact")
        .arg("api::tests::loopback_api_never_sends_its_token_to_an_environment_proxy")
        .arg("--nocapture")
        .env(CHILD, "1")
        .env("HTTP_PROXY", proxy.url())
        .env("http_proxy", proxy.url())
        .env("ALL_PROXY", proxy.url())
        .env("all_proxy", proxy.url())
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .status()?;
    assert!(
        status.success(),
        "proxy-isolated child must connect directly"
    );
    assert!(
        proxy.requests().is_empty(),
        "proxy must not receive loopback credentials"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn sdk_trace_headers_are_forwarded_exactly_without_inventing_baggage() -> TestResult {
    #[derive(Debug)]
    struct NoNetwork;
    impl Transport for NoNetwork {
        fn send_envelope(&self, envelope: Envelope) {
            drop(envelope);
        }
    }
    let options = ClientOptions::default()
        .dsn("https://public@127.0.0.1/1")
        .transport(Arc::new(NoNetwork));
    let client = Arc::new(Client::from(options));
    let hub = Arc::new(Hub::new(Some(client), Arc::new(Scope::default())));
    let server = Holuto::new();
    server.pairing_code("ABCD1234");
    let mut api = api(&server)?;
    let expected = Hub::run(hub, || -> Result<Vec<(&'static str, String)>, Error> {
        let context = TransactionContext::continue_from_headers(
            "test",
            "test",
            [
                (
                    "sentry-trace",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-1",
                ),
                (
                    "baggage",
                    "sentry-release=trace-fixture,sentry-public_key=public",
                ),
            ],
        );
        let transaction = start_transaction(context);
        Hub::current().configure_scope(|scope| scope.set_span(Some(transaction.into())));
        let expected = Hub::current()
            .configure_scope(|scope| scope.iter_trace_propagation_headers().collect::<Vec<_>>());
        api.session(&telemetry())?;
        let _paired = Api::pair(server.url(), "ABCD1234")?;
        Ok(expected)
    })?;
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        ["/agent/session", "/agent/pair"]
    );
    for request in requests {
        assert!(
            request
                .sentry_trace
                .as_deref()
                .is_some_and(|trace| trace.starts_with("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-")),
            "HTTP request remains in the upstream trace"
        );
        let mut actual = request.trace_headers.clone();
        actual.sort_by(|left, right| left.0.cmp(&right.0));
        let mut expected = expected
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect::<Vec<_>>();
        expected.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(
            actual, expected,
            "every SDK propagation header is forwarded unchanged"
        );
        assert!(
            actual
                .iter()
                .all(|(_name, value)| !value.contains(server.token())),
            "trace headers never contain the device token"
        );
    }
    Ok(())
}

#[test]
fn transport_diagnostics_never_retain_peer_supplied_or_token_text() {
    let secret = "synthetic-secret-in-untrusted-error";
    let error = ApiError::from(ureq::Error::BadUri(secret.to_owned()));
    assert!(matches!(
        error,
        ApiError::Transport(super::TransportFailure::Protocol)
    ));
    assert_eq!(
        error.to_string(),
        "Błąd komunikacji z Holuto: Nieprawidłowa odpowiedź protokołu HTTP."
    );
    assert!(
        !format!("{error:?}").contains(secret),
        "Debug retains only an error category"
    );
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn pairing_is_unauthenticated_and_returns_a_redacted_credential() -> TestResult {
    let server = Holuto::new();
    server.pairing_code("ABCD1234");
    let paired = Api::pair(server.url(), "abcd-1234")?;
    assert_eq!(paired.printer_id, PRINTER_ID);
    assert_eq!(paired.token.as_str(), format!("{:0>42}A", 1));
    assert!(!format!("{paired:?}").contains(paired.token.as_str()));
    let requests = server.requests();
    let request = requests.first().ok_or("missing pairing request")?;
    assert_eq!(request.path, "/agent/pair");
    assert!(!request.authenticated);
    assert_eq!(request.serial, None);
    assert_eq!(request.body, json!({"code":"ABCD1234"}));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn a_malformed_pairing_code_is_refused_without_spending_a_backend_attempt() -> TestResult {
    let server = Holuto::new();
    for code in [
        "",
        "1234",
        "ABCD-123",
        "ABCD12345",
        "ABCD-123I",
        "ABCD-123O",
        "ĄBCD-1234",
    ] {
        let error = Api::pair(server.url(), code)
            .err()
            .ok_or("malformed code accepted")?;
        assert_eq!(
            error.to_string(),
            "Kod parowania ma 8 znaków z Holuto, na przykład ABCD-1234.",
            "{code:?}"
        );
    }
    assert_eq!(server.requests().len(), 0, "no request reached Holuto");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "boundary assertions panic while setup uses Result"
)]
fn pairing_rejects_malformed_credentials_without_exposing_them() -> TestResult {
    let server = Holuto::new();
    for body in [
        json!({"token":"private secret","printerId":PRINTER_ID}),
        json!({"token":"private-secret","printerId":"not-a-uuid"}),
    ] {
        server.fault(
            "POST",
            "/agent/pair",
            Fault::Body {
                status: 200,
                body: serde_json::to_vec(&body)?,
            },
        );
        let error = Api::pair(server.url(), "ABCD1234")
            .err()
            .ok_or("missing refusal")?;
        assert!(matches!(error, Error::Api(ApiError::Response(_))));
        assert!(!format!("{error:?} {error}").contains("private"));
    }
    Ok(())
}
