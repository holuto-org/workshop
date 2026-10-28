use std::fmt::Debug;

use fiscal::device::Vendor;
use serde_json::{Value, json};

use super::{validate_request, validate_response};
use crate::{
    api::model::{
        Job, JobSource, JobView, JobsPage, JobsSearch, Page, PrinterStatus, QueueForm, ResultForm,
        Telemetry, VatRateMapping,
    },
    testing::{JOB_ID, ORDER_ID, job_json, status_json, with},
};

#[test]
fn generated_nullable_response_fields_require_presence_and_keep_per_job_amount_validation() {
    let original = job_json();
    for field in [
        "allocationId",
        "buyerNip",
        "attempt",
        "receiptNumber",
        "issuedAt",
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
        validate_response("POST", "/agent/documents/search", 200, &json!({"items":[]})).is_err(),
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
        }],"nextCursor":"PRIVATE-CURSOR","previousCursor":null
    }))
    .unwrap();
    let view: JobView = serde_json::from_value(json!({
        "job":with(job_json(), &json!({"result":"PRIVATE-NOTE"})),
        "customerName":"PRIVATE-CUSTOMER", "vehiclePlate":"WA12345", "source":"device"
    }))
    .unwrap();
    let search: JobsSearch = serde_json::from_value(json!({
        "cursor":{"limit":50}, "filter":{}, "search":{"q":"PRIVATE-QUERY"}
    }))
    .unwrap();
    let records: [(&dyn Debug, &str); 14] = [
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
        (&view, "JobView"),
        (&search, "JobsSearch"),
        (&search.search, "JobSearch"),
    ];
    for (record, name) in records {
        assert_eq!(format!("{record:?}"), format!("{name} {{ .. }}"));
    }
    assert_eq!(format!("{:?}", job.state), "Taken");
    assert_eq!(format!("{:?}", slot.vat_rate), "X23");
    assert_eq!(format!("{:?}", result.state), "Done");
    assert_eq!(format!("{:?}", view.source), "Device");
}

#[test]
fn history_search_enforces_the_operation_body_and_required_nullable_view_fields() {
    let path = "/agent/jobs/search";
    let request = json!({
        "cursor":{"limit":50}, "filter":{"orderId":ORDER_ID,"state":"done"},
        "search":{"q":"Jan WA123"}, "sort":["-createdAt","-id"]
    });
    validate_request("POST", path, &request).unwrap();
    for changes in [
        json!({"sort":["customerName"]}),
        json!({"filter":{"printerId":ORDER_ID}}),
        json!({"search":{"q":"x".repeat(201)}}),
    ] {
        assert!(validate_request("POST", path, &with(request.clone(), &changes)).is_err());
    }
    assert!(validate_request("POST", path, &json!({})).is_err());
    let view = json!({
        "job":job_json(), "customerName":null, "vehiclePlate":null, "source":"staff"
    });
    let page = json!({"items":[view],"nextCursor":null,"previousCursor":null});
    validate_response("POST", path, 200, &page).unwrap();
    let parsed: JobsPage = serde_json::from_value(page).unwrap();
    assert_eq!(parsed.items[0].job.id, JOB_ID);
    assert_eq!(parsed.items[0].source, JobSource::Staff);
    assert!(parsed.items[0].job.issued_at.is_none());
    for pointer in ["/customerName", "/vehiclePlate", "/job/issuedAt"] {
        let (parent, field) = pointer.rsplit_once('/').unwrap();
        let mut missing = view.clone();
        let _removed = missing
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .unwrap()
            .remove(field);
        let page = json!({"items":[missing],"nextCursor":null,"previousCursor":null});
        assert!(
            validate_response("POST", path, 200, &page).is_err(),
            "{pointer}"
        );
        assert!(
            serde_json::from_value::<JobView>(missing).is_err(),
            "{pointer}"
        );
    }
    assert!(serde_json::from_value::<JobsPage>(json!({"items":[],"nextCursor":null})).is_err());
}
