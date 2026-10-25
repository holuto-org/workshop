use jiff::civil::date;

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
