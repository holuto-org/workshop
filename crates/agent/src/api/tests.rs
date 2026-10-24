//! Real HTTP exchanges with a local server.

use std::{
    mem,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use serde_json::{Value, json};
use tiny_http::{Header, Response, Server};
use uuid::Uuid;

use super::{Api, ApiError, Holuto, queue_form};
use crate::{
    api::model::{Outcome, ResultForm, Telemetry},
    documents::{BuyerNip, Document, ReceiptKind},
    testing::{
        ALLOCATION_ID, ATTEMPT, JOB_ID, ORDER_ID, PRINTER_ID, job_json, printer_json, status_json,
        with,
    },
};

#[derive(Debug)]
struct Recorded {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Value>,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Answers each request with the next reply, then stops.
struct Fake {
    url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    thread: Option<JoinHandle<()>>,
}

impl Fake {
    fn start(replies: Vec<(u16, Value)>) -> Self {
        let server = Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr().to_ip().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let thread = thread::Builder::new()
            .name("fake Holuto".to_owned())
            .spawn(move || {
                for (status, reply) in replies {
                    let Ok(Some(mut request)) = server.recv_timeout(Duration::from_secs(5)) else {
                        return;
                    };
                    let mut body = String::new();
                    let _length = request.as_reader().read_to_string(&mut body).unwrap();
                    recorded.lock().unwrap().push(Recorded {
                        method: request.method().to_string(),
                        url: request.url().to_owned(),
                        headers: request
                            .headers()
                            .iter()
                            .map(|header| (header.field.to_string(), header.value.to_string()))
                            .collect(),
                        body: (!body.is_empty()).then(|| serde_json::from_str(&body).unwrap()),
                    });
                    let response = Response::from_string(if reply.is_null() {
                        String::new()
                    } else {
                        reply.to_string()
                    })
                    .with_status_code(status)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
                    .with_header(Header::from_bytes("Location", "/steal-credential").unwrap());
                    request.respond(response).unwrap();
                }
            });
        let thread = thread.unwrap();
        Self {
            url,
            requests,
            thread: Some(thread),
        }
    }

    fn api(&self) -> Api {
        Api::new(&self.url, "secret", "DEMO0000001").unwrap()
    }

    /// Every request the server received.
    fn finish(mut self) -> Vec<Recorded> {
        self.thread.take().unwrap().join().unwrap();
        mem::take(&mut *self.requests.lock().unwrap())
    }
}

fn paired(api: &mut Api) {
    api.printer_id = Some(PRINTER_ID);
}

fn telemetry() -> Telemetry {
    Telemetry::new("DEMO0000001")
}

fn done_body() -> ResultForm {
    ResultForm {
        attempt: ATTEMPT,
        state: Outcome::Done,
        receipt_number: Some("1".to_owned()),
        issued_at: Some("2026-10-16T10:02:00Z".parse().unwrap()),
        result: None,
    }
}

#[test]
fn every_call_uses_the_permanent_token_and_clean_paths() {
    let done = with(
        job_json(),
        &json!({"state": "done", "receiptNumber": "1", "resolvedAt": "2026-10-16T10:02:00Z"}),
    );
    let fake = Fake::start(vec![
        (200, status_json()),
        (200, status_json()),
        (200, job_json()),
        (200, job_json()),
        (200, done),
        (204, Value::Null),
    ]);
    let mut api = fake.api();
    let _printer = api.session(&telemetry()).unwrap();
    let _printer = api.heartbeat(&telemetry()).unwrap();
    assert_eq!(api.take().unwrap().map(|job| job.id), Some(JOB_ID));
    assert_eq!(api.job(JOB_ID).unwrap().id, JOB_ID);
    let _job = api.report(JOB_ID, &done_body()).unwrap();
    assert!(api.take().unwrap().is_none());
    let requests = fake.finish();
    let paths: Vec<_> = requests
        .iter()
        .map(|request| (request.method.as_str(), request.url.as_str()))
        .collect();
    let job = format!("/agent/jobs/{JOB_ID}");
    let result = format!("/agent/jobs/{JOB_ID}/result");
    assert_eq!(
        paths,
        [
            ("POST", "/agent/session"),
            ("POST", "/agent/heartbeat"),
            ("POST", "/agent/jobs/take"),
            ("GET", job.as_str()),
            ("POST", result.as_str()),
            ("POST", "/agent/jobs/take"),
        ]
    );
    for request in &requests {
        assert_eq!(request.header("Authorization"), Some("Bearer secret"));
        assert_eq!(
            request.header("App-Version"),
            Some(format!("agent/{}", env!("CARGO_PKG_VERSION")).as_str())
        );
    }
    assert!(
        requests[..2]
            .iter()
            .all(|request| request.header("X-Device-Serial").is_none())
    );
    assert!(
        requests[2..]
            .iter()
            .all(|request| request.header("X-Device-Serial") == Some("DEMO0000001"))
    );
    assert_eq!(
        requests[0].body,
        Some(serde_json::to_value(telemetry()).unwrap())
    );
    assert_eq!(requests[2].body, None);
    assert_eq!(
        requests[4].body,
        Some(json!({
            "attempt": ATTEMPT, "state": "done", "receiptNumber": "1",
            "issuedAt": "2026-10-16T10:02:00Z", "result": null
        }))
    );
}

/// One API call, its result reduced to success or the error.
type Call = fn(&mut Api) -> Result<(), ApiError>;

#[test]
fn a_rejected_token_fails_once_without_refresh_or_retry() {
    let calls: [Call; 5] = [
        |api| api.session(&telemetry()).map(drop),
        |api| api.heartbeat(&telemetry()).map(drop),
        |api| api.take().map(drop),
        |api| api.job(JOB_ID).map(drop),
        |api| api.report(JOB_ID, &done_body()).map(drop),
    ];
    for call in calls {
        let fake = Fake::start(vec![(401, json!({}))]);
        let mut api = fake.api();
        assert!(matches!(call(&mut api), Err(ApiError::Unauthorized)));
        assert_eq!(fake.finish().len(), 1);
    }
}

#[test]
fn redirects_and_server_errors_are_not_followed_or_retried() {
    for status in [302, 500] {
        let fake = Fake::start(vec![(
            status,
            json!({"detail": "Serwer niedostępny", "code": "x"}),
        )]);
        let error = fake.api().take().unwrap_err();
        assert!(
            matches!(&error, ApiError::Status { status: got, .. } if *got == status),
            "{error}"
        );
        assert_eq!(fake.finish().len(), 1);
    }
}

#[test]
fn a_problem_detail_reaches_the_operator() {
    let fake = Fake::start(vec![(
        409,
        json!({
            "type": "about:blank", "title": "Conflict", "status": 409, "code": "x",
            "detail": "Numer drukarki jest już sparowany."
        }),
    )]);
    let error = fake.api().session(&telemetry()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Holuto odpowiedziało błędem 409: Numer drukarki jest już sparowany."
    );
    let _requests = fake.finish();
}

#[test]
fn a_malformed_or_foreign_printer_session_is_rejected() {
    let printer = printer_json();
    for payload in [
        Value::Null,
        json!({}),
        printer.clone(),
        with(
            status_json(),
            &json!({"printer": with(printer.clone(), &json!({"deviceSerial": "OTHER"}))}),
        ),
        with(status_json(), &json!({"isOnline": 1})),
        with(
            status_json(),
            &json!({"printer": with(printer.clone(), &json!({"id": 7}))}),
        ),
        with(
            status_json(),
            &json!({"printer": with(
                printer.clone(),
                &json!({"vatSlots": [{"slot": true, "vatRate": "23"}]})
            )}),
        ),
        with(
            status_json(),
            &json!({"printer": with(
                printer.clone(),
                &json!({"vatSlots": [{"slot": 16, "vatRate": "23"}]})
            )}),
        ),
        with(
            status_json(),
            &json!({"printer": with(printer.clone(), &json!({"maxItemNameLength": 0}))}),
        ),
        with(
            status_json(),
            &json!({"printer": with(printer.clone(), &json!({"maxLinesPerReceipt": "500"}))}),
        ),
        with(
            status_json(),
            &json!({"printer": with(printer.clone(), &json!({"maxCompanyNameLength": 0}))}),
        ),
        with(
            status_json(),
            &json!({"printer": with(printer, &json!({"allowedCharacters": ""}))}),
        ),
        {
            let mut status = status_json();
            drop(
                status["printer"]
                    .as_object_mut()
                    .unwrap()
                    .remove("allowedCharacters"),
            );
            status
        },
    ] {
        let fake = Fake::start(vec![(200, payload.clone())]);
        assert!(
            matches!(fake.api().session(&telemetry()), Err(ApiError::Response(_))),
            "{payload}"
        );
        let _requests = fake.finish();
    }
}

#[test]
fn a_session_for_another_printer_than_before_is_rejected() {
    let other = with(
        status_json(),
        &json!({"printer": with(printer_json(), &json!({"id": Uuid::from_u128(99)}))}),
    );
    let fake = Fake::start(vec![(200, status_json()), (200, other)]);
    let mut api = fake.api();
    let _printer = api.session(&telemetry()).unwrap();
    assert!(matches!(
        api.heartbeat(&telemetry()),
        Err(ApiError::Response(_))
    ));
    let _requests = fake.finish();
}

#[test]
fn nullable_printer_metadata_is_accepted() {
    let nulls = json!({
        "deviceSerial": null, "model": null, "firmware": null, "appVersion": null,
        "maxItemNameLength": null, "maxLinesPerReceipt": null, "maxCompanyNameLength": null,
        "allowedCharacters": null
    });
    let status = with(
        status_json(),
        &json!({"lastSeenAt": null, "printer": with(printer_json(), &nulls)}),
    );
    let fake = Fake::start(vec![(200, status)]);
    assert_eq!(fake.api().session(&telemetry()).unwrap().id, PRINTER_ID);
    let _requests = fake.finish();
}

#[test]
fn a_job_that_does_not_match_the_contract_is_rejected() {
    let bad_line = json!([{"name": "x", "quantity": 1, "unitPrice": "10.00", "total": "10.00", "vatRate": "23", "vatSlot": 0}]);
    for payload in [
        json!({}),
        with(job_json(), &json!({"lines": bad_line})),
        with(job_json(), &json!({"id": 7})),
        with(job_json(), &json!({"payments": 2})),
        with(job_json(), &json!({"kind": "refund"})),
        with(job_json(), &json!({"advances": null})),
        with(
            job_json(),
            &json!({"lines": [with(job_json()["lines"][0].clone(), &json!({"vatRate": "22"}))]}),
        ),
        {
            let mut job = job_json();
            drop(job.as_object_mut().unwrap().remove("attempt"));
            job
        },
    ] {
        let fake = Fake::start(vec![(200, payload.clone())]);
        assert!(
            matches!(fake.api().take(), Err(ApiError::Response(_))),
            "{payload}"
        );
        let _requests = fake.finish();
    }
}

#[test]
fn a_status_for_another_job_and_an_unechoed_result_are_rejected() {
    let fake = Fake::start(vec![
        (200, with(job_json(), &json!({"id": PRINTER_ID}))),
        (
            200,
            with(
                job_json(),
                &json!({"state": "unknown", "attempt": PRINTER_ID}),
            ),
        ),
        (200, with(job_json(), &json!({"state": "taken"}))),
    ]);
    let mut api = fake.api();
    paired(&mut api);
    assert!(matches!(api.job(JOB_ID), Err(ApiError::Response(_))));
    let unknown = ResultForm {
        state: Outcome::Unknown,
        receipt_number: None,
        issued_at: None,
        result: Some("x".to_owned()),
        ..done_body()
    };
    assert!(matches!(
        api.report(JOB_ID, &unknown),
        Err(ApiError::Response(_))
    ));
    assert!(matches!(
        api.report(JOB_ID, &done_body()),
        Err(ApiError::Response(_))
    ));
    let _requests = fake.finish();
}

#[test]
fn an_invalid_serial_is_rejected_never_encoded_or_truncated() {
    for serial in [
        "",
        " ABC",
        "ABC ",
        "A\r\nB",
        "A\u{0}B",
        "Żółw",
        &"9".repeat(33),
    ] {
        assert!(
            Api::new("https://api.example", "secret", serial).is_err(),
            "{serial:?}"
        );
    }
}

#[test]
fn the_serial_is_sent_exactly() {
    let fake = Fake::start(vec![(204, Value::Null)]);
    let mut api = Api::new(&fake.url, "secret", "AB/1 %2").unwrap();
    assert!(api.take().unwrap().is_none());
    assert_eq!(fake.finish()[0].header("X-Device-Serial"), Some("AB/1 %2"));
}

fn document(kind: &str, order: Uuid, number: i64, allocation: Option<Uuid>, amount: &str) -> Value {
    json!({
        "kind": kind, "orderId": order, "orderNumber": number, "allocationId": allocation,
        "amount": amount, "customerName": null, "occurredAt": "2026-10-16T10:00:00Z"
    })
}

#[test]
fn documents_and_queue_follow_the_contract() {
    for (kind, allocation) in [("advance", Some(ALLOCATION_ID)), ("sale", None)] {
        let page = json!({"items": [document(kind, ORDER_ID, 10, allocation, "250.00")], "nextCursor": null});
        let queued = with(
            job_json(),
            &json!({"state": "queued", "attempt": null, "takenAt": null, "buyerNip": "5260250274"}),
        );
        let fake = Fake::start(vec![(200, page), (201, queued)]);
        let mut api = fake.api();
        let listed = api.documents(None).unwrap();
        let [item]: [Document; 1] = listed.items.try_into().unwrap();
        assert_eq!((item.order_number, item.allocation_id), (10, allocation));
        let nip = BuyerNip::parse("5260250274").unwrap();
        let _job = api.queue(&queue_form(&item, Some(nip)).unwrap()).unwrap();
        let requests = fake.finish();
        assert_eq!(requests[0].url, "/agent/documents?limit=100");
        assert_eq!(
            (requests[1].method.as_str(), requests[1].url.as_str()),
            ("POST", "/agent/jobs")
        );
        assert!(
            requests
                .iter()
                .all(|request| request.header("X-Device-Serial") == Some("DEMO0000001"))
        );
        assert_eq!(
            requests[1].body,
            Some(
                json!({"kind": kind, "orderId": ORDER_ID, "allocationId": allocation, "buyerNip": "5260250274"})
            )
        );
    }
}

#[test]
fn an_opaque_cursor_is_passed_back_unchanged_and_encoded() {
    let cursor = "a/b+c d&limit=1?#%2F=ż";
    let first = json!({"items": [document("sale", ORDER_ID, 10, None, "100000000.00")], "nextCursor": cursor});
    let last = json!({"items": [document("sale", JOB_ID, 11, None, "120.00")], "nextCursor": null});
    let fake = Fake::start(vec![(200, first), (200, last)]);
    let mut api = fake.api();
    let page = api.documents(None).unwrap();
    assert_eq!(page.next_cursor.as_deref(), Some(cursor));
    assert_eq!(
        page.items[0].amount, 10_000_000_000,
        "listed even above the printer limit"
    );
    assert!(api.documents(Some(cursor)).unwrap().next_cursor.is_none());
    let requests = fake.finish();
    assert_eq!(
        requests[1].url,
        "/agent/documents?limit=100&cursor=a%2Fb%2Bc%20d%26limit%3D1%3F%23%252F%3D%C5%BC"
    );
}

#[test]
fn a_malformed_page_is_rejected_whole() {
    let good = document("sale", ORDER_ID, 10, None, "250.00");
    for page in [
        json!({"items": []}),
        json!({"items": [], "nextCursor": ""}),
        json!({"items": [], "nextCursor": 7}),
        json!({"items": vec![good.clone(); 101], "nextCursor": null}),
        json!({"items": [good.clone(), with(good.clone(), &json!({"amount": "NaN"}))], "nextCursor": "next"}),
        json!({"items": [with(good.clone(), &json!({"kind": "refund"}))], "nextCursor": null}),
        json!({"items": [with(good.clone(), &json!({"kind": "advance"}))], "nextCursor": null}),
        json!({"items": [with(good.clone(), &json!({"allocationId": ATTEMPT}))], "nextCursor": null}),
        json!({"items": [with(good.clone(), &json!({"amount": "-1.00"}))], "nextCursor": null}),
        json!({"items": [with(good.clone(), &json!({"amount": "not-a-number"}))], "nextCursor": null}),
        json!({"items": [with(good, &json!({"occurredAt": "2026-10-16"}))], "nextCursor": null}),
    ] {
        let fake = Fake::start(vec![(200, page.clone())]);
        assert!(
            matches!(
                fake.api().documents(Some("previous")),
                Err(ApiError::Response(_))
            ),
            "{page}"
        );
        assert_eq!(fake.finish().len(), 1);
    }
}

#[test]
fn an_empty_cursor_is_rejected_before_sending() {
    let fake = Fake::start(vec![]);
    assert!(fake.api().documents(Some("")).is_err());
    assert_eq!(fake.finish().len(), 0);
}

#[test]
fn an_inconsistent_document_is_never_queued() {
    for (kind, allocation) in [
        (ReceiptKind::Sale, Some(ATTEMPT)),
        (ReceiptKind::Advance, None),
    ] {
        let mut document: Document =
            serde_json::from_value(document("sale", ORDER_ID, 10, None, "1.00")).unwrap();
        document.kind = kind;
        document.allocation_id = allocation;
        assert!(queue_form(&document, None).is_err(), "{kind:?}");
    }
}
