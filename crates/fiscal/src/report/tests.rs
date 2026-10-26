use jiff::civil::date;
use serde_json::error::Category;

use super::Report;

#[test]
fn persisted_reports_keep_their_exact_kind_and_date_fields() {
    for (report, json) in [
        (Report::Daily, "{\"kind\":\"daily\"}"),
        (
            Report::Monthly {
                year: 2026,
                month: 6,
            },
            "{\"kind\":\"monthly\",\"year\":2026,\"month\":6}",
        ),
        (
            Report::Periodic {
                from: date(2026, 1, 1),
                to: date(2026, 6, 30),
                summary: true,
            },
            "{\"kind\":\"periodic\",\"from\":\"2026-01-01\",\
             \"to\":\"2026-06-30\",\"summary\":true}",
        ),
        (
            Report::Periodic {
                from: date(2024, 2, 29),
                to: date(2024, 2, 29),
                summary: false,
            },
            "{\"kind\":\"periodic\",\"from\":\"2024-02-29\",\
             \"to\":\"2024-02-29\",\"summary\":false}",
        ),
    ] {
        assert_eq!(serde_json::to_string(&report).unwrap(), json);
        assert_eq!(serde_json::from_str::<Report>(json).unwrap(), report);
    }
}

#[test]
fn persisted_reports_reject_unknown_fields_in_every_kind() {
    for json in [
        r#"{"kind":"daily","extra":true}"#,
        r#"{"kind":"monthly","year":2026,"month":6,"extra":true}"#,
        r#"{"kind":"periodic","from":"2026-01-01","to":"2026-06-30","summary":true,"extra":true}"#,
    ] {
        assert!(
            serde_json::from_str::<Report>(json).is_err(),
            "unknown report fields must fail closed: {json}"
        );
    }
}

#[test]
fn corrupt_persisted_reports_cannot_silently_acquire_a_different_request() {
    for json in [
        r#"{"kind":"annual"}"#,
        r#"{"kind":"monthly","year":2026}"#,
        r#"{"kind":"periodic","from":"2026-01-01","to":"2026-01-02"}"#,
        r#"{"kind":"periodic","from":"2026-02-29","to":"2026-03-01","summary":true}"#,
        r#"{"kind":"periodic","from":"2026-01-01","to":null,"summary":false}"#,
        r#"{"kind":"daily","kind":"monthly","year":2026,"month":6}"#,
    ] {
        assert_eq!(
            serde_json::from_str::<Report>(json).unwrap_err().classify(),
            Category::Data,
            "{json}"
        );
    }
}
