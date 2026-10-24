use std::{collections::BTreeSet, fmt::Debug};

use serde_json::{Value, json};

use super::{validate_request, validate_response};
use crate::{
    api::{
        Api,
        model::{Job, Outcome, Page, PrinterStatus, QueueForm, ResultForm, Telemetry, VatSlotRate},
    },
    documents::{Document, ReceiptKind},
    testing::{Holuto, JOB_ID, ORDER_ID, job_json, status_json, telemetry},
};

#[test]
fn every_client_operation_matches_the_vendored_method_path_and_success_statuses() {
    let server = Holuto::new();
    let mut api = Api::new(server.url(), server.token(), "TEST-001").unwrap();
    let telemetry = telemetry("TEST-001");
    api.session(&telemetry).unwrap();
    api.heartbeat(&telemetry).unwrap();
    let page = api.documents(None).unwrap();
    assert_eq!(page.items, Vec::<Document>::new());
    assert_eq!(page.next_cursor, None);
    assert_eq!(api.take().unwrap(), None);
    api.queue(&QueueForm {
        allocation_id: None,
        buyer_nip: None,
        kind: ReceiptKind::Sale,
        order_id: ORDER_ID,
    })
    .unwrap();
    let taken = api.take().unwrap().expect("queued job");
    assert_eq!(api.job(taken.id).unwrap(), taken);
    api.report(
        taken.id,
        &ResultForm {
            attempt: taken.attempt.unwrap(),
            issued_at: None,
            receipt_number: None,
            result: Some("Odmowa przed drukiem".into()),
            state: Outcome::Failed,
        },
    )
    .unwrap();
    let observed: BTreeSet<_> = server
        .requests()
        .into_iter()
        .map(|request| {
            let path = request.path.split('?').next().unwrap();
            let path = path.replace(&taken.id.to_string(), "{jobId}");
            (request.method, path)
        })
        .collect();
    let document: Value =
        serde_json::from_str(include_str!("../../../../../openapi/agent.json")).unwrap();
    let expected = [
        ("POST", "/agent/session", vec!["200"]),
        ("POST", "/agent/heartbeat", vec!["200"]),
        ("GET", "/agent/documents", vec!["200"]),
        ("POST", "/agent/jobs", vec!["201"]),
        ("POST", "/agent/jobs/take", vec!["200", "204"]),
        ("GET", "/agent/jobs/{jobId}", vec!["200"]),
        ("POST", "/agent/jobs/{jobId}/result", vec!["200"]),
    ];
    assert_eq!(
        observed,
        expected
            .iter()
            .map(|(method, path, _)| (String::from(*method), String::from(*path)))
            .collect()
    );
    for (method, path, statuses) in expected {
        let responses = document["paths"][path][method.to_ascii_lowercase()]["responses"]
            .as_object()
            .unwrap();
        assert_eq!(
            responses
                .keys()
                .filter(|status| status.starts_with('2'))
                .map(String::as_str)
                .collect::<Vec<_>>(),
            statuses,
            "{method} {path}"
        );
    }
}

#[test]
fn generated_nullable_response_fields_require_presence_and_keep_per_job_amount_validation() {
    let original = job_json();
    for field in [
        "allocationId",
        "buyerNip",
        "attempt",
        "receiptNumber",
        "result",
        "takenAt",
        "resolvedAt",
    ] {
        let mut job = original.clone();
        let _removed = job.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<Job>(job).is_err(),
            "missing {field}"
        );
    }
    let mut malformed_amount = original;
    malformed_amount["total"] = json!("not a decimal");
    assert_eq!(
        serde_json::from_value::<Job>(malformed_amount)
            .unwrap()
            .total,
        "not a decimal"
    );
    let mut status = status_json();
    let _removed = status["printer"]
        .as_object_mut()
        .unwrap()
        .remove("deviceSerial");
    assert!(
        serde_json::from_value::<PrinterStatus>(status).is_err(),
        "missing nullable printer field"
    );
    assert!(
        serde_json::from_value::<Page>(json!({"items":[]})).is_err(),
        "missing nullable cursor"
    );
}

#[test]
fn fake_validation_enforces_schema_patterns_enums_and_nullable_presence() {
    let valid = job_json();
    let path = format!("/agent/jobs/{JOB_ID}");
    validate_response("GET", &path, 200, &valid).unwrap();
    for (field, value) in [
        ("total", json!("garbage")),
        ("state", json!("invented")),
        ("kind", json!("invented")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert!(
            validate_response("GET", &path, 200, &invalid).is_err(),
            "invalid {field}"
        );
    }
    assert!(
        validate_response("GET", "/agent/documents", 200, &json!({"items":[]})).is_err(),
        "required nullable cursor"
    );
    assert!(
        validate_request("POST", "/agent/session", &json!({})).is_err(),
        "required telemetry"
    );
    assert!(
        validate_request("DELETE", &path, &Value::Null).is_err(),
        "unknown operation"
    );
    assert!(
        validate_response("POST", "/agent/jobs/take", 204, &valid).is_err(),
        "204 must have no body"
    );
}

#[test]
fn generated_page_preserves_the_domain_decimal_conversion_in_both_directions() {
    let value = json!({
        "items": [{
            "kind": "sale", "allocationId": null, "orderId": ORDER_ID,
            "orderNumber": 42, "amount": "12.34", "customerName": null,
            "occurredAt": "2026-10-24T10:00:00Z"
        }],
        "nextCursor": null
    });
    let page: Page = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(page.items.first().unwrap().amount, 1234);
    assert_eq!(serde_json::to_value(page).unwrap(), value);
}

#[test]
fn wire_debug_never_exposes_customer_receipt_or_telemetry_values_but_keeps_enum_states_readable() {
    let mut value = job_json();
    value["companyName"] = json!("PRIVATE-COMPANY");
    value["buyerNip"] = json!("5260250274");
    value["total"] = json!("123.45");
    value["result"] = json!("PRIVATE-NOTE");
    value["receiptNumber"] = json!("PRIVATE-RECEIPT");
    value["lines"][0]["name"] = json!("PRIVATE-LINE");
    value["lines"][0]["quantity"] = json!("987.654321");
    value["payments"][0]["amount"] = json!("123.45");
    value["advances"] = json!([{"vatSlot":0,"vatRate":"23","amount":"33.21"}]);
    let job: Job = serde_json::from_value(value).unwrap();
    let mut value = status_json();
    value["printer"]["name"] = json!("PRIVATE-PRINTER-NAME");
    let status: PrinterStatus = serde_json::from_value(value).unwrap();
    let mut telemetry = Telemetry::new("PRIVATE-DEVICE-SERIAL");
    telemetry.model = "PRIVATE-MODEL".into();
    let slot: VatSlotRate = serde_json::from_value(json!({"slot":0,"vatRate":"23"})).unwrap();
    let queue: QueueForm = serde_json::from_value(json!({
        "orderId":ORDER_ID,"kind":"sale","allocationId":null,"buyerNip":"5260250274"
    }))
    .unwrap();
    let result: ResultForm = serde_json::from_value(json!({
        "attempt":job.attempt,"state":"done","receiptNumber":"PRIVATE-RECEIPT",
        "issuedAt":"2026-10-24T10:00:00Z","result":"PRIVATE-NOTE"
    }))
    .unwrap();
    let page: Page = serde_json::from_value(json!({
        "items":[{
            "kind":"sale","allocationId":null,"orderId":ORDER_ID,"orderNumber":42,
            "amount":"987.65","customerName":"PRIVATE-CUSTOMER","occurredAt":"2026-10-24T10:00:00Z"
        }],"nextCursor":"PRIVATE-CURSOR"
    }))
    .unwrap();
    let records: [(&dyn Debug, &str); 11] = [
        (&job, "Job"),
        (job.lines.first().unwrap(), "JobLine"),
        (job.advances.first().unwrap(), "JobAdvance"),
        (job.payments.first().unwrap(), "JobPayment"),
        (&status, "PrinterStatus"),
        (&status.printer, "PrinterInfo"),
        (&telemetry, "Telemetry"),
        (&slot, "VatSlotRate"),
        (&queue, "QueueForm"),
        (&result, "ResultForm"),
        (&page, "Page"),
    ];
    for (record, name) in records {
        assert_eq!(format!("{record:?}"), format!("{name} {{ .. }}"));
    }
    assert_eq!(format!("{:?}", job.state), "Taken");
    assert_eq!(format!("{:?}", slot.vat_rate), "X23");
    assert_eq!(format!("{:?}", result.state), "Done");
}
