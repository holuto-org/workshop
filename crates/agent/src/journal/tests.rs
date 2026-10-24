use super::{JobResult, MAX_REASON_LENGTH, note};

#[test]
fn a_result_text_is_bounded_and_free_of_control_characters() {
    assert_eq!(note(&"x".repeat(5000)).chars().count(), MAX_REASON_LENGTH);
    assert_eq!(note("a\u{0}b\r\nc\td"), "ab\nc\td");
    assert_eq!(note(" \u{7} "), "Brak opisu błędu.");
    assert_eq!(
        JobResult::failed(&"ż".repeat(3000)),
        JobResult::Failed {
            reason: "ż".repeat(2000)
        }
    );
}
