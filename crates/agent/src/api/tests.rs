use std::{
    env,
    error::Error as StdError,
    fmt, fs, io,
    process::Command,
    sync::{Arc, Mutex},
};

use fiscal::device::Vendor;
use sentry_core::{
    Client, ClientOptions, Envelope, Hub, Scope, TransactionContext, Transport, start_transaction,
};
use serde_json::json;
use tracing::{
    Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id},
    subscriber,
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};
use uuid::Uuid;

use super::{
    Api, ApiError,
    model::{Outcome, ResultForm},
};
use crate::{
    Error,
    documents::{BuyerNip, Document, ReceiptKind},
    testing::{
        ATTEMPT, Fault, Holuto, JOB_ID, ORDER_ID, PRINTER_ID, job_json, status_json, telemetry,
        with,
    },
};

type TestResult = Result<(), Box<dyn StdError>>;
fn api(server: &Holuto) -> Result<Api, Error> {
    Api::new(server.url(), server.token(), "DEM0000000001")
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn vendor_binding_round_trips_all_vendors_and_refuses_device_swaps() -> TestResult {
    for (vendor, name) in [
        (Vendor::Posnet, "posnet"),
        (Vendor::Novitus, "novitus"),
        (Vendor::Elzab, "elzab"),
    ] {
        let server = Holuto::new();
        let mut api = api(&server)?;
        let mut telemetry = telemetry();
        telemetry.vendor = vendor;
        api.session(&telemetry)?;
        api.heartbeat()?;
        assert_eq!(
            server.requests().first().ok_or("no session")?.body["vendor"],
            name
        );
        telemetry.vendor = if vendor == Vendor::Posnet {
            Vendor::Novitus
        } else {
            Vendor::Posnet
        };
        assert!(matches!(api.session(&telemetry),
                Err(ApiError::Status { status: 403, detail: Some(detail) })
                    if detail == "Połącz drukarkę marki zapisanej dla tego parowania."));
        telemetry.vendor = vendor;
        telemetry.unique_number = "OTH0000000001".to_owned();
        assert!(matches!(api.session(&telemetry),
                Err(ApiError::Status { status: 403, detail: Some(detail) })
                    if detail == "Ten token jest sparowany z inną fizyczną drukarką."));
    }
    Ok(())
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
    let error = api
        .report(job.id, &form)
        .err()
        .ok_or("lost reply was accepted")?;
    assert!(matches!(
        error,
        ApiError::Transport(super::TransportFailure::Protocol)
    ));
    assert_eq!(format!("{error:?}"), "Transport(Protocol)");
    api.report(job.id, &form)?;
    assert_eq!(server.job(job.id)["state"], "done");
    assert_eq!(server.job(job.id)["receiptNumber"], "123");
    assert_eq!(server.job(job.id)["attempt"], json!(form.attempt));
    assert_eq!(server.job(job.id)["result"], serde_json::Value::Null);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn a_result_reply_that_does_not_echo_the_report_is_not_accepted() -> TestResult {
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
    let echo = with(
        server.job(JOB_ID),
        &json!({"state": "done", "receiptNumber": "123"}),
    );
    let path = format!("/agent/jobs/{JOB_ID}/result");
    server.fault(
        "POST",
        &path,
        Fault::Body {
            status: 200,
            body: serde_json::to_vec(&echo)?,
        },
    );
    api.report(job.id, &form)?;
    for changes in [
        json!({"id": Uuid::from_u128(98)}),
        json!({"attempt": Uuid::from_u128(98)}),
        json!({"state": "failed", "receiptNumber": null}),
        json!({"receiptNumber": "124"}),
    ] {
        server.fault(
            "POST",
            &path,
            Fault::Body {
                status: 200,
                body: serde_json::to_vec(&with(echo.clone(), &changes))?,
            },
        );
        assert!(
            matches!(api.report(job.id, &form), Err(ApiError::Response(message))
                if message == "wynik zadania nie został zapisany zgodnie z raportem"),
            "{changes}"
        );
    }
    // Without a receipt number, only the echoed state shows whether the outcome was recorded.
    let unknown = ResultForm {
        issued_at: None,
        receipt_number: None,
        result: Some("Sprawdź wydruk.".to_owned()),
        state: Outcome::Unknown,
        ..form
    };
    for (state, accepted) in [("unknown", true), ("taken", false)] {
        let reply = with(
            server.job(JOB_ID),
            &json!({"state": state, "receiptNumber": null, "result": "Sprawdź wydruk."}),
        );
        server.fault(
            "POST",
            &path,
            Fault::Body {
                status: 200,
                body: serde_json::to_vec(&reply)?,
            },
        );
        let reported = api.report(job.id, &unknown);
        if accepted {
            reported?;
        } else {
            assert!(
                matches!(reported, Err(ApiError::Response(message))
                    if message == "wynik zadania nie został zapisany zgodnie z raportem"),
                "an unchanged {state} job did not record the outcome"
            );
        }
    }
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
fn malformed_json_is_a_contract_error_not_a_connection_error() -> TestResult {
    let server = Holuto::new();
    let mut api = api(&server)?;
    server.fault(
        "POST",
        "/agent/session",
        Fault::Body {
            status: 200,
            body: b"not json".to_vec(),
        },
    );
    assert!(
        matches!(api.session(&telemetry()), Err(ApiError::Response(message))
        if message == "expected ident at line 1 column 2")
    );
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
        matches!(&error, ApiError::Status { status: 409, detail: Some(detail) }
        if detail == "bad [REDACTED]")
    );
    assert_eq!(format!("{error:?}"), "Status { status: 409, .. }");
    assert_eq!(
        error.to_string(),
        "Holuto odpowiedziało błędem 409: bad [REDACTED]"
    );
    Ok(())
}

#[derive(Clone, Default)]
struct Routes(Arc<Mutex<Vec<String>>>);

impl Visit for Routes {
    fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "route" {
            self.0.lock().unwrap().push(value.to_owned());
        }
    }
}

impl<S: Subscriber> Layer<S> for Routes {
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _context: Context<'_, S>) {
        attrs.record(&mut self.clone());
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
#[expect(
    clippy::too_many_lines,
    reason = "Exact request and telemetry tables cover every API route together."
)]
fn every_endpoint_uses_device_credentials_and_correct_serial_binding() -> TestResult {
    let server = Holuto::new();
    let routes = Routes::default();
    let _subscriber = subscriber::set_default(tracing_subscriber::registry().with(routes.clone()));
    let _id = server.enqueue(job_json());
    let mut api = api(&server)?;
    let mut profile = telemetry();
    profile.capabilities.allowed_characters = "AĄa 1".to_owned();
    api.session(&profile)?;
    api.heartbeat()?;
    let _page = api.documents(None)?;
    api.queue(&super::model::QueueForm {
        allocation_id: None,
        buyer_nip: None,
        kind: ReceiptKind::Sale,
        order_id: Uuid::from_u128(1000),
    })?;
    let job = api.take()?.ok_or("missing job")?;
    let _remote = api.job(job.id)?;
    api.report(
        job.id,
        &ResultForm {
            attempt: job.attempt.ok_or("no attempt")?,
            issued_at: None,
            receipt_number: None,
            result: Some("Odmowa przed drukiem.".to_owned()),
            state: Outcome::Failed,
        },
    )?;
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
            "/agent/jobs",
            "/agent/jobs/take",
            "/agent/jobs/00000000-0000-0000-0000-000000000007",
            "/agent/jobs/00000000-0000-0000-0000-000000000007/result"
        ]
    );
    assert_eq!(
        requests
            .iter()
            .map(|request| request.method.as_str())
            .collect::<Vec<_>>(),
        ["POST", "POST", "GET", "POST", "POST", "GET", "POST"]
    );
    assert_eq!(
        requests
            .iter()
            .map(|request| request.body.clone())
            .collect::<Vec<_>>(),
        [
            json!({
                "appVersion": env!("CARGO_PKG_VERSION"),
                "vendor": "posnet", "uniqueNumber": "DEM0000000001",
                "model": "POSNET TEMO ONLINE", "firmware": "32.01", "vatRates": [],
                "capabilities": {
                    "advanceDeductions": true, "zeroTotalReceipts": true,
                    "maxItemNameLength": 80, "maxCompanyNameLength": 40,
                    "maxLinesPerReceipt": 500, "maxQuantityDecimals": 8,
                    "allowedCharacters": "AĄa 1"
                }
            }),
            serde_json::Value::Null,
            serde_json::Value::Null,
            json!({"kind":"sale","orderId":Uuid::from_u128(1000),
                "allocationId":null,"buyerNip":null}),
            serde_json::Value::Null,
            serde_json::Value::Null,
            json!({"attempt":job.attempt,"state":"failed","receiptNumber":null,
                "issuedAt":null,"result":"Odmowa przed drukiem."})
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
            request.unique_number.as_deref(),
            if index == 0 {
                None
            } else {
                Some("DEM0000000001")
            }
        );
    }
    assert_eq!(
        *routes
            .0
            .lock()
            .map_err(|_poison| "poisoned route capture")?,
        [
            "/agent/session",
            "/agent/heartbeat",
            "/agent/documents",
            "/agent/jobs",
            "/agent/jobs/take",
            "/agent/jobs/{id}",
            "/agent/jobs/{id}/result"
        ]
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
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        ["/agent/session"],
        "401 causes no refresh or retry"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "test assertions intentionally panic while setup errors use Result"
)]
fn redirects_never_forward_device_credentials() -> TestResult {
    let server = Holuto::new();
    let target = Holuto::new();
    server.fault(
        "POST",
        "/agent/session",
        Fault::Redirect(format!("{}/steal-credentials", target.url())),
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
        server
            .requests()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        ["/agent/session"],
        "redirect target must receive no request"
    );
    assert_eq!(target.requests(), []);
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
fn malformed_nullable_session_fields_and_foreign_unique_number_are_rejected() -> TestResult {
    for changes in [
        json!({"uniqueNumber":"OTHER"}),
        json!({"capabilities":{"maxItemNameLength":0}}),
        json!({"capabilities":{"maxQuantityDecimals":9}}),
        json!({"capabilities":{"allowedCharacters":""}}),
    ] {
        let server = Holuto::new();
        let mut status = status_json();
        if let Some(capabilities) = changes.get("capabilities") {
            status["printer"]["capabilities"] =
                with(status["printer"]["capabilities"].clone(), capabilities);
        } else {
            status["printer"] = with(status["printer"].clone(), &changes);
        }
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
        .remove("uniqueNumber");
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
fn opaque_cursor_is_encoded_once() -> TestResult {
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
    for unique_number in [
        "",
        " UNIQUE_NUMBER",
        "UNIQUE_NUMBER ",
        "ŁÓDŹ",
        "bad\nserial",
    ] {
        assert!(
            matches!(
                Api::new(server.url(), server.token(), unique_number),
                Err(Error::Invalid(_))
            ),
            "unique_number identity is never canonicalized or truncated"
        );
    }
    assert!(
        server.requests().is_empty(),
        "invalid unique_number causes no request"
    );
    Ok(())
}

#[test]
fn cleartext_or_credentialed_remote_api_urls_are_refused_before_any_request() {
    let server = Holuto::new();
    let refused = "Adres API wymaga https (http tylko dla localhost), bez danych logowania, \
                   zapytania i fragmentu.";
    // `.invalid` never resolves, so a missing guard fails without leaving the machine.
    for url in [
        "http://holuto.invalid",
        "https://user:secret@holuto.invalid",
        "https://holuto.invalid?next=1",
    ] {
        assert!(
            matches!(
                Api::new(url, server.token(), "DEM0000000001"),
                Err(Error::Invalid(message)) if message == refused
            ),
            "{url}"
        );
        assert!(
            matches!(
                Api::pair(url, "ABCD1234"),
                Err(Error::Invalid(message)) if message == refused
            ),
            "{url}"
        );
    }
    assert_eq!(server.requests().len(), 0);
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
        fs::write(
            env::var_os("HOLUTO_AGENT_PROXY_OUTCOME").ok_or("missing outcome")?,
            b"direct authenticated session",
        )?;
        return Ok(());
    }
    let proxy = Holuto::new();
    let temporary = tempfile::tempdir()?;
    let outcome = temporary.path().join("outcome");
    #[expect(
        clippy::disallowed_methods,
        reason = "proxy environment is isolated in a child test process"
    )]
    let status = Command::new(env::current_exe()?)
        .arg("--exact")
        .arg("api::tests::loopback_api_never_sends_its_token_to_an_environment_proxy")
        .arg("--nocapture")
        .env(CHILD, "1")
        .env("HOLUTO_AGENT_PROXY_OUTCOME", &outcome)
        .env("HTTP_PROXY", proxy.url())
        .env("http_proxy", proxy.url())
        .env("HTTPS_PROXY", proxy.url())
        .env("https_proxy", proxy.url())
        .env("ALL_PROXY", proxy.url())
        .env("all_proxy", proxy.url())
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .status()?;
    assert!(
        status.success(),
        "proxy-isolated child must connect directly"
    );
    assert_eq!(fs::read(outcome)?, b"direct authenticated session");
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
                .trace_headers
                .iter()
                .any(|(name, trace)| name == "sentry-trace"
                    && trace.starts_with("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-")),
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
    assert_eq!(request.unique_number, None);
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
fn pairing_accepts_only_well_formed_credentials_and_never_echoes_rejected_ones() -> TestResult {
    let server = Holuto::new();
    // An unpadded base64url 32-byte token: 43 characters whose last one carries 2 bits.
    let stem = format!("private_-{}", "x".repeat(33));
    let valid = format!("{stem}w");
    let pair = |body: &serde_json::Value| -> Result<_, Box<dyn StdError>> {
        server.fault(
            "POST",
            "/agent/pair",
            Fault::Body {
                status: 200,
                body: serde_json::to_vec(body)?,
            },
        );
        Ok(Api::pair(server.url(), "ABCD1234"))
    };
    let paired = pair(&json!({"token": valid, "printerId": PRINTER_ID}))??;
    assert_eq!(paired.token.as_str(), valid);
    assert_eq!(paired.printer_id, PRINTER_ID);
    for body in [
        json!({"token": "private secret", "printerId": PRINTER_ID}),
        json!({"token": stem, "printerId": PRINTER_ID}),
        json!({"token": format!("{stem}x"), "printerId": PRINTER_ID}),
        json!({"token": valid.replace('_', "+"), "printerId": PRINTER_ID}),
        json!({"token": valid, "printerId": Uuid::nil()}),
        json!({"token": valid, "printerId": "private-not-a-uuid"}),
    ] {
        let error = pair(&body)?.err().ok_or("malformed credential accepted")?;
        assert_eq!(
            error.to_string(),
            "Nieprawidłowa odpowiedź Holuto (nieprawidłowe dane parowania).",
            "{body}"
        );
        assert!(!format!("{error:?} {error}").contains("private"));
    }
    Ok(())
}

fn listed_document() -> serde_json::Value {
    json!({
        "kind":"sale", "orderId":Uuid::from_u128(10), "orderNumber":42,
        "allocationId":null, "amount":"12.34", "customerName":null,
        "occurredAt":"2026-10-24T10:00:00Z", "dueAt":"2026-10-24T10:00:00Z",
        "latestAt":"2026-10-24T10:00:00Z", "isOverdue":false
    })
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup."
)]
fn response_body_limit_accepts_exact_bound_and_rejects_one_more_byte() -> TestResult {
    let server = Holuto::new();
    let mut api = api(&server)?;
    for status in [200, 503] {
        let body = if status == 200 {
            status_json()
        } else {
            json!({"detail":"Awaria Holuto."})
        };
        let mut bytes = serde_json::to_vec(&body)?;
        bytes.resize(4 * 1024 * 1024, b' ');
        server.fault(
            "POST",
            "/agent/session",
            Fault::Body {
                status,
                body: bytes.clone(),
            },
        );
        let response = api.session(&telemetry());
        if status == 200 {
            response.map_err(|error| error.to_string())?;
            assert_eq!(api.printer_id(), Some(PRINTER_ID));
        } else {
            assert!(
                matches!(response, Err(ApiError::Status {status:503, detail:Some(detail)})
                if detail == "Awaria Holuto.")
            );
        }
        bytes.push(b' ');
        server.fault(
            "POST",
            "/agent/session",
            Fault::Body {
                status,
                body: bytes,
            },
        );
        assert!(
            matches!(api.session(&telemetry()), Err(ApiError::Response(message))
            if message == "przekroczony limit rozmiaru odpowiedzi")
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup."
)]
fn document_page_bounds_and_identity_are_checked_before_publication() -> TestResult {
    let server = Holuto::new();
    let mut api = api(&server)?;
    api.session(&telemetry())?;
    let document = listed_document();
    server.page(
        None,
        json!({"items":vec![document.clone();100],"nextCursor":"opaque-next"}),
    );
    let page = api.documents(None)?;
    assert_eq!(
        serde_json::to_value(&page)?,
        json!({
            "items":vec![document.clone();100], "nextCursor":"opaque-next"
        })
    );
    assert_eq!(page.items.first().ok_or("missing item")?.amount, 1234);
    for (items, cursor, expected) in [
        (
            vec![document.clone(); 101],
            json!(null),
            "strona przekracza limit",
        ),
        (
            vec![document.clone()],
            json!(""),
            "pusty kursor następnej strony",
        ),
        (
            vec![
                document.clone(),
                with(document.clone(), &json!({"orderNumber":0})),
            ],
            json!(null),
            "numer zlecenia musi być dodatni",
        ),
        (
            vec![with(document.clone(), &json!({"kind":"advance"}))],
            json!(null),
            "zaliczka bez identyfikatora alokacji",
        ),
        (
            vec![with(document, &json!({"allocationId":Uuid::from_u128(12)}))],
            json!(null),
            "sprzedaż z identyfikatorem alokacji",
        ),
    ] {
        server.fault(
            "GET",
            "/agent/documents",
            Fault::Body {
                status: 200,
                body: serde_json::to_vec(&json!({"items":items,"nextCursor":cursor}))?,
            },
        );
        assert!(
            matches!(api.documents(None), Err(ApiError::Response(message)) if message == expected)
        );
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup."
)]
fn a_rejected_session_cannot_replace_the_previously_bound_printer() -> TestResult {
    let server = Holuto::new();
    let mut api = api(&server)?;
    api.session(&telemetry())?;
    let mut reply = status_json();
    reply["printer"]["id"] = json!(Uuid::from_u128(99));
    server.fault(
        "POST",
        "/agent/session",
        Fault::Body {
            status: 200,
            body: serde_json::to_vec(&reply)?,
        },
    );
    assert!(
        matches!(api.session(&telemetry()), Err(ApiError::Response(message))
        if message == "parowanie przypisane do innej drukarki")
    );
    assert_eq!(api.printer_id(), Some(PRINTER_ID));
    api.session(&telemetry())?;
    assert_eq!(api.printer_id(), Some(PRINTER_ID));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP boundary assertions use Result for setup."
)]
fn api_path_prefix_and_advance_queue_fields_reach_the_peer_unchanged() -> TestResult {
    let server = Holuto::new();
    server.fault(
        "POST",
        "/prefix/agent/session",
        Fault::Body {
            status: 200,
            body: serde_json::to_vec(&status_json())?,
        },
    );
    let mut api = Api::new(
        &format!("{}/prefix///", server.url()),
        server.token(),
        "DEM0000000001",
    )?;
    api.session(&telemetry())?;
    server.fault(
        "POST",
        "/prefix/agent/jobs",
        Fault::Body {
            status: 201,
            body: serde_json::to_vec(&job_json())?,
        },
    );
    let document: Document = serde_json::from_value(with(
        listed_document(),
        &json!({"kind":"advance","allocationId":Uuid::from_u128(12)}),
    ))?;
    let form = super::queue_form(&document, Some(BuyerNip::parse(" 5260250274 ")?))?;
    api.queue(&form)?;
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| (request.method.as_str(), request.path.as_str()))
            .collect::<Vec<_>>(),
        [
            ("POST", "/prefix/agent/session"),
            ("POST", "/prefix/agent/jobs")
        ]
    );
    assert_eq!(
        requests.last().ok_or("missing queue")?.body,
        json!({"kind":"advance","orderId":Uuid::from_u128(10),
            "allocationId":Uuid::from_u128(12),"buyerNip":"5260250274"})
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "TLS boundary assertions use Result for setup."
)]
fn an_untrusted_tls_peer_never_receives_http_credentials() -> TestResult {
    use rustls::{
        ServerConfig, ServerConnection, StreamOwned,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject as _},
    };
    use std::{io::Read as _, net::TcpListener, thread, time::Duration};
    let certificate =
        CertificateDer::from_pem_slice(include_bytes!("fixtures/untrusted-localhost.pem"))?;
    let key =
        PrivateKeyDer::from_pem_slice(include_bytes!("fixtures/untrusted-localhost-key.pem"))?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let peer = thread::Builder::new()
        .name("untrusted TLS peer".to_owned())
        .spawn(move || {
            let (socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(Duration::from_secs(5)))?;
            socket.set_write_timeout(Some(Duration::from_secs(5)))?;
            let connection = ServerConnection::new(Arc::new(config)).map_err(io::Error::other)?;
            let mut stream = StreamOwned::new(connection, socket);
            let mut plaintext = [0; 1024];
            Ok::<_, io::Error>(stream.read(&mut plaintext))
        })?;
    let mut api = Api::new(
        &format!("https://{address}"),
        "synthetic-TLS-token",
        "DEM0000000001",
    )?;
    let result = api.session(&telemetry());
    let read = peer.join().map_err(|_panic| "TLS peer panicked")??;
    assert!(
        matches!(
            result,
            Err(ApiError::Transport(super::TransportFailure::Tls))
        ),
        "TLS certificate refusal: {result:?}; peer: {read:?}"
    );
    assert!(
        matches!(read, Err(_) | Ok(0)),
        "certificate refusal must happen before any HTTP plaintext"
    );
    let error = result.err().ok_or("untrusted TLS certificate accepted")?;
    assert_eq!(format!("{error:?}"), "Transport(Tls)");
    assert_eq!(
        error.to_string(),
        "Błąd komunikacji z Holuto: Nie można zweryfikować bezpiecznego połączenia TLS."
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "HTTP failures must retain exact errors and server state."
)]
fn every_authenticated_endpoint_propagates_http_failures_without_retry_or_mutation() -> TestResult {
    for (operation, method, path) in [
        ("session", "POST", "/agent/session"),
        ("heartbeat", "POST", "/agent/heartbeat"),
        ("documents", "GET", "/agent/documents"),
        ("queue", "POST", "/agent/jobs"),
        ("take", "POST", "/agent/jobs/take"),
        (
            "job",
            "GET",
            "/agent/jobs/00000000-0000-0000-0000-000000000007",
        ),
        (
            "report",
            "POST",
            "/agent/jobs/00000000-0000-0000-0000-000000000007/result",
        ),
    ] {
        for status in [401, 403, 408, 429, 500, 503] {
            let server = Holuto::new();
            let previous = job_json();
            server.insert(previous.clone());
            let mut api = api(&server)?;
            api.session(&telemetry())?;
            server.fault(
                method,
                path,
                Fault::Problem {
                    status,
                    code: "unavailable".to_owned(),
                    detail: format!("Błąd {}.", server.token()),
                },
            );
            let response = match operation {
                "session" => api.session(&telemetry()),
                "heartbeat" => api.heartbeat(),
                "documents" => api.documents(None).map(drop),
                "queue" => api.queue(&super::model::QueueForm {
                    allocation_id: None,
                    buyer_nip: None,
                    kind: ReceiptKind::Sale,
                    order_id: ORDER_ID,
                }),
                "take" => api.take().map(drop),
                "job" => api.job(JOB_ID).map(drop),
                "report" => api.report(
                    JOB_ID,
                    &ResultForm {
                        attempt: ATTEMPT,
                        issued_at: None,
                        receipt_number: None,
                        result: Some("Przed wysłaniem.".to_owned()),
                        state: Outcome::Failed,
                    },
                ),
                _ => return Err("unknown literal operation".into()),
            };
            let error = response.err().ok_or("HTTP failure accepted")?;
            if status == 401 {
                assert!(matches!(error, ApiError::Unauthorized), "{operation}");
            } else {
                assert!(
                    matches!(&error,ApiError::Status {status:actual,detail:Some(detail)}
                    if *actual == status && detail == "Błąd [REDACTED]."),
                    "{operation}: {error:?}"
                );
            }
            assert!(!format!("{error:?} {error}").contains(server.token()));
            let expected_path = if operation == "documents" {
                "/agent/documents?limit=100"
            } else {
                path
            };
            assert_eq!(
                server
                    .requests()
                    .iter()
                    .map(|request| (request.method.as_str(), request.path.as_str()))
                    .collect::<Vec<_>>(),
                [("POST", "/agent/session"), (method, expected_path)],
                "{operation}/{status}: no retries or token refresh"
            );
            assert_eq!(server.job(JOB_ID), previous, "{operation}/{status}");
        }
    }
    Ok(())
}

#[test]
fn sdk_transport_failures_keep_their_safe_operator_category_without_waiting_for_real_deadlines() {
    use super::TransportFailure;
    for (external, expected) in [
        (
            ureq::Error::Timeout(ureq::Timeout::Global),
            TransportFailure::Timeout,
        ),
        (ureq::Error::ConnectionFailed, TransportFailure::Connection),
        (ureq::Error::HostNotFound, TransportFailure::Dns),
        (ureq::Error::InvalidProxyUrl, TransportFailure::Proxy),
        (
            ureq::Error::ConnectProxyFailed("PRIVATE-PROXY".to_owned()),
            TransportFailure::Proxy,
        ),
        (ureq::Error::Tls("PRIVATE-TLS"), TransportFailure::Tls),
        (
            ureq::Error::BadUri("PRIVATE-URL".to_owned()),
            TransportFailure::Protocol,
        ),
        (
            ureq::Error::Io(io::Error::new(io::ErrorKind::TimedOut, "PRIVATE-IO")),
            TransportFailure::Timeout,
        ),
        (
            ureq::Error::Io(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "PRIVATE-IO",
            )),
            TransportFailure::ConnectionRefused,
        ),
        (
            ureq::Error::Io(io::Error::new(io::ErrorKind::InvalidData, "PRIVATE-IO")),
            TransportFailure::Protocol,
        ),
        (
            ureq::Error::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "PRIVATE-IO")),
            TransportFailure::Protocol,
        ),
        (
            ureq::Error::Io(io::Error::new(io::ErrorKind::BrokenPipe, "PRIVATE-IO")),
            TransportFailure::Io,
        ),
    ] {
        let error = ApiError::from(external);
        assert!(matches!(&error,ApiError::Transport(actual) if *actual == expected));
        assert!(!format!("{error:?} {error}").contains("PRIVATE"));
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Untrusted HTTP headers must be bounded by the production client."
)]
fn an_oversized_response_header_is_a_contract_error_not_a_protocol_error() -> TestResult {
    let server = Holuto::new();
    let mut api = api(&server)?;
    server.fault(
        "POST",
        "/agent/session",
        Fault::Redirect("/".repeat(128 * 1024)),
    );
    assert!(
        matches!(api.session(&telemetry()),Err(ApiError::Response(message))
        if message == "przekroczony limit rozmiaru odpowiedzi")
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The complete legal VAT table must survive HTTP unchanged."
)]
fn session_uploads_all_seven_legal_vat_letters_and_refuses_an_eighth() -> TestResult {
    use fiscal::device::{Device, VatKind, VatRate};
    use posnet::capabilities::capabilities;
    let server = Holuto::new();
    let mut api = api(&server)?;
    let mut profile = telemetry();
    let vat = [2300, 800, 500, 0, 0, 800, 2300]
        .into_iter()
        .enumerate()
        .map(|(slot, rate)| {
            Ok::<_, Box<dyn StdError>>(VatRate {
                slot: u8::try_from(slot)?,
                kind: if slot == 4 {
                    VatKind::Exempt
                } else {
                    VatKind::Rate { basis_points: rate }
                },
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let device = Device {
        vendor: Vendor::Posnet,
        model: "POSNET TEMO ONLINE".to_owned(),
        firmware: "32.01".to_owned(),
        unique_number: "DEM0000000001".to_owned(),
        problems: Vec::new(),
        vat,
        capabilities: capabilities(),
    };
    assert_eq!(profile.describe(&device), Vec::<String>::new());
    api.session(&profile)?;
    let expected = json!([
        {"letter":"A","vatRate":"23"},{"letter":"B","vatRate":"8"},
        {"letter":"C","vatRate":"5"},{"letter":"D","vatRate":"0"},
        {"letter":"E","vatRate":"zw"},{"letter":"F","vatRate":"8"},{"letter":"G","vatRate":"23"}
    ]);
    assert_eq!(
        server.requests().last().ok_or("missing session")?.body["vatRates"],
        expected
    );
    for count in [7, 8, 9] {
        let mut status = status_json();
        let mut rates = expected.as_array().ok_or("missing VAT table")?.clone();
        rates.resize(count, json!({"letter":"F","vatRate":"8"}));
        status["printer"]["vatRates"] = json!(rates);
        server.fault(
            "POST",
            "/agent/session",
            Fault::Body {
                status: 200,
                body: serde_json::to_vec(&status)?,
            },
        );
        let result = api.session(&profile);
        if count == 7 {
            result?;
        } else {
            let expected = "nieprawidłowe litery VAT";
            assert!(matches!(result,Err(ApiError::Response(message)) if message == expected));
        }
    }
    Ok(())
}
