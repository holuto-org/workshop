use std::fs;

use posnet::{money::Grosze, receipt::PaymentForm};
use uuid::Uuid;

use super::{HistoryEntry, append, load};
use crate::testing::state_dir;

fn entry(id: u128) -> HistoryEntry {
    HistoryEntry {
        job_id: Uuid::from_u128(id),
        printed_at: "2026-10-16T10:00:00Z".parse().unwrap(),
        receipt_number: Some(id.to_string()),
        total: Grosze::new(100).unwrap(),
        payments: vec![
            (PaymentForm::Card, Grosze::new(40).unwrap()),
            (PaymentForm::Credit, Grosze::new(60).unwrap()),
        ],
        items: vec!["Olej".to_owned()],
    }
}

#[test]
fn newest_receipts_first_each_job_once() {
    let (_temporary, dir) = state_dir();
    for id in [1, 2, 1] {
        append(&dir, &entry(id)).unwrap();
    }
    assert_eq!(load(&dir).unwrap(), [entry(2), entry(1)]);
}

#[test]
fn a_line_torn_by_a_crash_is_skipped_and_never_swallows_the_next_entry() {
    let (_temporary, dir) = state_dir();
    append(&dir, &entry(1)).unwrap();
    let path = dir.path().join("history.jsonl");
    let mut torn = fs::read(&path).unwrap();
    torn.extend_from_slice(b"{\"jobId\":");
    fs::write(&path, torn).unwrap();
    append(&dir, &entry(2)).unwrap();
    assert_eq!(load(&dir).unwrap(), [entry(2), entry(1)]);
}

#[test]
fn at_most_five_hundred_are_listed() {
    let (_temporary, dir) = state_dir();
    let lines: String = (0..501)
        .map(|id| serde_json::to_string(&entry(id)).unwrap() + "\n")
        .collect();
    fs::write(dir.path().join("history.jsonl"), lines).unwrap();
    let listed = load(&dir).unwrap();
    assert_eq!(listed.len(), 500);
    assert_eq!(listed[0], entry(500));
}
