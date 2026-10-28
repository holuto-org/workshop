//! Paired-printer history with body-only search and criteria-bound, bidirectional cursors.

use std::cmp::Ordering;

use icu_normalizer::DecomposingNormalizerBorrowed;
use icu_properties::{CodePointSetData, props::Diacritic};
use jiff::Timestamp;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{PRINTER_ID, State, cursor_page, invalid_limit, problem_detail};

#[derive(Clone, Copy)]
pub(super) struct Key {
    id: Uuid,
    created_at: Timestamp,
}

fn invalid(pointer: &str, code: &str) -> (u16, Value) {
    (
        400,
        json!({
            "type": "about:blank", "title": "Holuto", "status": 400,
            "code": "fiscal_invalid_input",
            "detail": problem_detail("fiscal_invalid_input"),
            "errors": [{"pointer": pointer, "code": code, "detail": problem_detail(code)}]
        }),
    )
}

/// Recognized invalid criteria get the same field problems as the service. Valid
/// requests still pass through the fake's `OpenAPI` validator before routing.
pub(super) fn invalid_request(body: &Value) -> Option<(u16, Value)> {
    if body.pointer("/filter/printerId").is_some() {
        return Some(invalid("/filter/printerId", "unknown_field"));
    }
    if let Some(q) = body.pointer("/search/q").and_then(Value::as_str) {
        let code = if q.chars().nth(128).is_some() {
            Some("too_long")
        } else if q.trim().is_empty() {
            Some("blank")
        } else if q.chars().any(char::is_control) {
            Some("invalid_characters")
        } else {
            None
        };
        if let Some(code) = code {
            return Some(invalid("/search/q", code));
        }
    }
    if !body.get("cursor").is_some_and(Value::is_object) {
        return Some(invalid("/cursor", "required"));
    }
    invalid_limit(body, invalid)
}

fn folded(value: &str) -> String {
    let normalized = DecomposingNormalizerBorrowed::new_nfd().normalize(value);
    normalized
        .chars()
        .filter(|c| !CodePointSetData::new::<Diacritic>().contains(*c))
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ł' => 'l',
            'ø' => 'o',
            other => other,
        })
        .collect()
}

fn matches(job: &Value, metadata: &Value, words: &[&str]) -> bool {
    if words.is_empty() {
        return true;
    }
    let mut fields = vec![job["orderNumber"].to_string()];
    fields.extend(job["receiptNumber"].as_str().map(folded));
    for field in ["customerName", "vehiclePlate"] {
        fields.extend(metadata[field].as_str().map(folded));
    }
    if let Some(lines) = job["lines"].as_array() {
        fields.extend(
            lines
                .iter()
                .filter_map(|line| line["name"].as_str())
                .map(folded),
        );
    }
    words
        .iter()
        .all(|word| fields.iter().any(|field| field.contains(word)))
}

#[expect(
    clippy::unwrap_used,
    reason = "job fixtures must carry valid timestamps"
)]
fn key(id: Uuid, job: &Value) -> Key {
    Key {
        id,
        created_at: job["createdAt"].as_str().unwrap().parse().unwrap(),
    }
}

fn compare(a: Key, b: Key, sort: &[&str]) -> Ordering {
    for key in sort {
        let order = match key.trim_start_matches('-') {
            "createdAt" => a.created_at.cmp(&b.created_at),
            _ => a.id.cmp(&b.id),
        };
        let order = if key.starts_with('-') {
            order.reverse()
        } else {
            order
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    // Explicit sorts need a unique tie-breaker too.
    b.id.cmp(&a.id)
}

pub(super) fn search(state: &mut State, body: &Value) -> (u16, Value) {
    let cursor = body.pointer("/cursor/cursor").and_then(Value::as_str);
    if let Some(page) = state.jobs_pages.get(&cursor.map(str::to_owned)) {
        return (200, page.clone());
    }
    let query = folded(
        body.pointer("/search/q")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let words: Vec<_> = query
        .split([' ', ','])
        .filter(|word| !word.is_empty())
        .collect();
    let sort: Vec<_> = body["sort"]
        .as_array()
        .filter(|sort| !sort.is_empty())
        .map_or_else(
            || vec!["-createdAt", "-id"],
            |sort| sort.iter().filter_map(Value::as_str).collect(),
        );
    let printer = json!(PRINTER_ID);
    let filter_state = body
        .pointer("/filter/state")
        .filter(|value| !value.is_null());
    let filter_order = body
        .pointer("/filter/orderId")
        .filter(|value| !value.is_null());
    let rows = state
        .jobs
        .iter()
        .filter_map(|(id, job)| {
            let metadata = state.jobs_metadata.get(id).unwrap_or(&Value::Null);
            (job["printerId"] == printer
                && filter_state.is_none_or(|state| job["state"] == *state)
                && filter_order.is_none_or(|order| job["orderId"] == *order)
                && matches(job, metadata, &words))
            .then(|| {
                let source = if metadata["source"].is_null() {
                    json!("staff")
                } else {
                    metadata["source"].clone()
                };
                let view = json!({
                    "job": job,
                    "customerName": metadata["customerName"],
                    "vehiclePlate": metadata["vehiclePlate"],
                    "source": source
                });
                (key(*id, job), view)
            })
        })
        .collect();
    cursor_page(
        &mut state.jobs_cursors,
        "history",
        body,
        rows,
        |a, b| compare(a, b, &sort),
        invalid,
    )
}
