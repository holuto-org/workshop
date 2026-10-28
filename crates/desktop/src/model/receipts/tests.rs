use jiff::{civil::date, tz::TimeZone};

use super::{day_label, issue_label};

#[test]
fn day_headings_share_one_shape_and_show_the_year_only_for_other_years() {
    let today = date(2026, 10, 28);
    assert_eq!(
        [
            date(2026, 10, 28),
            date(2026, 10, 27),
            date(2026, 10, 26),
            date(2025, 12, 31),
        ]
        .map(|day| day_label(day, today)),
        [
            "Dziś · 28.10",
            "Wczoraj · 27.10",
            "Poniedziałek · 26.10",
            "31.12.2025"
        ]
    );
}

#[test]
fn an_issue_date_is_kept_when_it_differs_from_the_creation_day() {
    let at = |day: i8, hour: i8, minute: i8| {
        date(2026, 10, day)
            .at(hour, minute, 0, 0)
            .to_zoned(TimeZone::system())
            .unwrap()
            .timestamp()
    };
    let created = date(2026, 10, 27);
    assert_eq!(issue_label(at(27, 11, 15), created), "11:15");
    assert_eq!(
        issue_label(at(28, 0, 10), created),
        "28.10.2026 00:10",
        "printed after midnight: the fiscal date differs from the heading"
    );
}
