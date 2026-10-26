use std::fmt::Debug;

use fiscal::device::Vendor;
use serde_json::{Value, json};

use super::{validate_request, validate_response};
use crate::{
    api::model::{Job, Page, PrinterStatus, QueueForm, ResultForm, Telemetry, VatRateMapping},
    testing::{JOB_ID, ORDER_ID, job_json, status_json},
};

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
    assert_eq!(
        validate_response("GET", "/agent/jobs/not-a-uuid", 200, &valid),
        Err("operation absent from OpenAPI: GET /agent/jobs/not-a-uuid".to_owned())
    );
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
    value["advances"] = json!([{"vatLetter": "A","vatRate":"23","amount":"33.21"}]);
    let job: Job = serde_json::from_value(value).unwrap();
    let mut value = status_json();
    value["printer"]["name"] = json!("PRIVATE-PRINTER-NAME");
    let status: PrinterStatus = serde_json::from_value(value).unwrap();
    let mut telemetry = Telemetry::new("PRIVATE-DEVICE-UNIQUE_NUMBER", Vendor::Posnet);
    telemetry.model = "PRIVATE-MODEL".into();
    let slot: VatRateMapping =
        serde_json::from_value(json!({"letter": "A","vatRate":"23"})).unwrap();
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
            "amount":"987.65","customerName":"PRIVATE-CUSTOMER","occurredAt":"2026-10-24T10:00:00Z",
        "dueAt": "2026-10-24T10:00:00Z",
        "latestAt": "2026-10-24T10:00:00Z",
        "isOverdue": false
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
        (&slot, "VatRateMapping"),
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
