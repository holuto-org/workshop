use std::{
    collections::BTreeSet,
    error::Error,
    fmt::{self, Debug},
};

use fiscal::{device::Vendor, receipt::PaymentForm};
use serde::{
    Deserializer,
    de::{self, DeserializeOwned, Visitor},
    forward_to_deserialize_any,
};
use serde_json::{Value, json};

use super::{validate_request, validate_response};
use crate::{
    api::model::{
        Job, JobAdvance, JobLine, JobPayment, JobState, Outcome, Pairing, PrinterCapabilities,
        PrinterInfo, PrinterStatus, ReceiptKind, ResultForm, Telemetry, VatCode, VatLetter,
        VatRateMapping,
    },
    testing::{JOB_ID, PRINTER_ID, job_json, status_json},
};

/// The field or variant names a derived `Deserialize` asks its deserializer for.
fn names<T: DeserializeOwned>() -> BTreeSet<&'static str> {
    #[derive(Debug)]
    struct Stop;
    impl fmt::Display for Stop {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("names recorded")
        }
    }
    impl Error for Stop {}
    impl de::Error for Stop {
        fn custom<M: fmt::Display>(_: M) -> Self {
            Self
        }
    }
    struct Names<'a>(&'a mut &'static [&'static str]);
    impl<'de> Deserializer<'de> for Names<'_> {
        type Error = Stop;

        forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
            option unit unit_struct newtype_struct seq tuple tuple_struct map identifier
            ignored_any
        }

        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Stop> {
            Err(Stop)
        }

        fn deserialize_struct<V: Visitor<'de>>(
            self,
            _: &'static str,
            fields: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Stop> {
            *self.0 = fields;
            Err(Stop)
        }

        fn deserialize_enum<V: Visitor<'de>>(
            self,
            _: &'static str,
            variants: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Stop> {
            *self.0 = variants;
            Err(Stop)
        }
    }
    let mut names: &'static [&'static str] = &[];
    let _stopped = T::deserialize(Names(&mut names));
    names.iter().copied().collect()
}

fn schema(name: &str) -> Value {
    let document: Value =
        serde_json::from_str(include_str!("../../../../../openapi/agent.json")).unwrap();
    document["components"]["schemas"][name].clone()
}

fn strings(value: &Value) -> BTreeSet<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

/// Responses: a contract change that renames, removes or makes optional a field the
/// client reads, or adds a value to an enum it decodes, fails here, not in the field.
#[test]
fn response_types_read_only_required_schema_fields_and_decode_every_enum_value() {
    for (fields, component) in [
        (names::<Job>(), "AgentJob"),
        (names::<JobLine>(), "ReceiptLine"),
        (names::<JobAdvance>(), "AdvanceDeduction"),
        (names::<JobPayment>(), "ReceiptPayment"),
        (names::<PrinterStatus>(), "AgentSession"),
        (names::<PrinterInfo>(), "AgentPrinter"),
        (names::<PrinterCapabilities>(), "PrinterCapabilities"),
        (names::<VatRateMapping>(), "VatRateMapping"),
        (names::<Pairing>(), "AgentPaired"),
    ] {
        let fields: BTreeSet<String> = fields.into_iter().map(str::to_owned).collect();
        let required = strings(&schema(component)["required"]);
        assert!(!fields.is_empty(), "{component}");
        assert!(
            fields.is_subset(&required),
            "{component} reads {:?}, which the contract does not require",
            fields.difference(&required).collect::<Vec<_>>()
        );
    }
    for (variants, component) in [
        (names::<JobState>(), "FiscalJobState"),
        (names::<Outcome>(), "Outcome"),
        (names::<VatCode>(), "VatRate"),
        (names::<VatLetter>(), "FiscalVatLetter"),
        (names::<ReceiptKind>(), "FiscalReceiptKind"),
        (names::<PaymentForm>(), "FiscalPaymentForm"),
        (names::<Vendor>(), "FiscalVendor"),
    ] {
        let variants: BTreeSet<String> = variants.into_iter().map(str::to_owned).collect();
        assert_eq!(variants, strings(&schema(component)["enum"]), "{component}");
    }
}

#[test]
fn nullable_response_fields_require_presence_and_keep_per_job_amount_validation() {
    let original = job_json();
    for field in [
        "allocationId",
        "buyerNip",
        "attempt",
        "receiptNumber",
        "issuedAt",
        "result",
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
    let mut unresolved = valid.clone();
    let _removed = unresolved.as_object_mut().unwrap().remove("result");
    assert!(
        validate_response("POST", "/agent/jobs/take", 200, &unresolved).is_err(),
        "required nullable result"
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
    value["printer"]["uniqueNumber"] = json!("PRIVATE-UNIQUE-NUMBER");
    let status: PrinterStatus = serde_json::from_value(value).unwrap();
    let mut telemetry = Telemetry::new("PRIVATE-DEVICE-UNIQUE_NUMBER", Vendor::Posnet);
    telemetry.model = "PRIVATE-MODEL".into();
    let slot: VatRateMapping =
        serde_json::from_value(json!({"letter": "A","vatRate":"23"})).unwrap();
    let result = ResultForm {
        attempt: job.attempt.unwrap(),
        issued_at: Some("2026-10-24T10:00:00Z".parse().unwrap()),
        receipt_number: Some("PRIVATE-RECEIPT".into()),
        result: Some("PRIVATE-NOTE".into()),
        state: Outcome::Done,
    };
    let pairing: Pairing = serde_json::from_value(json!({
        "printerId":PRINTER_ID, "token":"PRIVATE-TOKEN"
    }))
    .unwrap();
    let records: [(&dyn Debug, &str); 10] = [
        (&job, "Job"),
        (job.lines.first().unwrap(), "JobLine"),
        (job.advances.first().unwrap(), "JobAdvance"),
        (job.payments.first().unwrap(), "JobPayment"),
        (&status, "PrinterStatus"),
        (&status.printer, "PrinterInfo"),
        (&telemetry, "Telemetry"),
        (&slot, "VatRateMapping"),
        (&result, "ResultForm"),
        (&pairing, "Pairing"),
    ];
    for (record, name) in records {
        assert_eq!(format!("{record:?}"), format!("{name} {{ .. }}"));
    }
    assert_eq!(format!("{:?}", job.state), "Taken");
    assert_eq!(format!("{:?}", slot.vat_rate), "Rate23");
    assert_eq!(format!("{:?}", result.state), "Done");
}
