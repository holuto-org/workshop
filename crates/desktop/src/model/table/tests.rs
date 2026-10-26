use std::array;

use agent::documents::ReceiptKind;
use fiscal::receipt::PaymentForm;

use super::*;
use proptest::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Tagged {
    key: u8,
    identity: usize,
}
impl Row for Tagged {
    fn cells(&self) -> [SharedString; COLUMNS] {
        array::from_fn(|_| self.key.to_string().into())
    }

    fn compare(&self, other: &Self, _: usize) -> Ordering {
        self.key.cmp(&other.key)
    }
}

proptest! {
    #[test]
    fn sorting_is_a_stable_permutation_and_preserves_selection(
        keys in prop::collection::vec(0_u8..20, 1..80),
        descending in any::<bool>(),
        selected in any::<usize>(),
    ) {
        let selected = selected % keys.len();
        let mut table = Table::new();
        let _old = table.set(keys.into_iter().enumerate().map(|(identity, key)| {
            Tagged { key, identity }
        }).collect());
        let new_selection = table.sort(Sort { column: 0, descending }, Some(selected));
        let mut identities: Vec<_> = table.items().iter().map(|row| row.identity).collect();
        identities.sort_unstable();
        prop_assert_eq!(identities, (0..table.items().len()).collect::<Vec<_>>());
        prop_assert_eq!(
            new_selection.and_then(|index| table.items().get(index)).map(|row| row.identity),
            Some(selected)
        );
        for rows in table.items().windows(2) {
            if let [a, b] = rows {
                let ordered = if descending { a.key >= b.key } else { a.key <= b.key };
                prop_assert!(ordered, "rows follow selected direction");
                if a.key == b.key { prop_assert!(a.identity < b.identity); }
            }
        }
    }
}

#[test]
fn polish_names_sort_in_the_operator_alphabet() {
    let mut table = Table::new();
    let _previous = table.set(
        [
            "Żak", "Źrebak", "Zenon", "Łukasz", "Lz", "ącki", "adam", "Ćma", "Czajka",
        ]
        .map(|name| Document {
            customer_name: Some(name.into()),
            ..document()
        })
        .to_vec(),
    );
    let _selection = table.sort(
        Sort {
            column: 2,
            descending: false,
        },
        None,
    );
    assert_eq!(
        table
            .items()
            .iter()
            .map(|row| row.customer_name.as_deref())
            .collect::<Vec<_>>(),
        [
            "adam", "ącki", "Czajka", "Ćma", "Lz", "Łukasz", "Zenon", "Źrebak", "Żak"
        ]
        .map(Some)
    );
}

#[test]
fn receipt_counters_sort_numerically_before_staff_entered_numbers() {
    let mut table = Table::new();
    let _previous = table.set(
        [
            Some("staff-7"),
            Some("10"),
            None,
            Some("9"),
            Some("2"),
            Some("02"),
        ]
        .map(|number| HistoryEntry {
            job_id: uuid::Uuid::nil(),
            printed_at: Timestamp::UNIX_EPOCH,
            receipt_number: number.map(str::to_owned),
            total: Grosze::ZERO,
            payments: Vec::new(),
            items: Vec::new(),
        })
        .to_vec(),
    );
    let _selection = table.sort(
        Sort {
            column: 2,
            descending: false,
        },
        None,
    );
    assert_eq!(
        table
            .items()
            .iter()
            .map(|row| row.receipt_number.as_deref())
            .collect::<Vec<_>>(),
        [
            None,
            Some("02"),
            Some("2"),
            Some("9"),
            Some("10"),
            Some("staff-7")
        ]
    );
}

#[test]
fn refreshing_a_page_keeps_the_selected_sort_direction() {
    let mut table = Table::new();
    let _old = table.set(vec![Tagged {
        key: 1,
        identity: 0,
    }]);
    let _selection = table.sort(
        Sort {
            column: 0,
            descending: true,
        },
        None,
    );
    let _old = table.set(vec![
        Tagged {
            key: 1,
            identity: 0,
        },
        Tagged {
            key: 3,
            identity: 1,
        },
        Tagged {
            key: 2,
            identity: 2,
        },
    ]);
    assert_eq!(
        table.items().iter().map(|row| row.key).collect::<Vec<_>>(),
        [3, 2, 1],
        "new data must not reset the operator's sort"
    );
}

#[test]
fn document_deadlines_sort_independently_and_overdue_is_explicit() {
    let mut document = document();
    let earlier = document.clone();
    document.latest_at = "2026-10-24T10:00:00Z".parse().unwrap();
    // Deadlines show the operator's local date; no single instant has one date in every zone.
    let local = document.latest_at.to_zoned(TimeZone::system());
    let date = format!(
        "{:02}.{:02}.{} {:02}:{:02}",
        local.day(),
        local.month(),
        local.year(),
        local.hour(),
        local.minute()
    );
    assert_eq!(document.compare(&earlier, 4), Ordering::Greater);
    // A bank-settled advance before its statutory limit is still due now.
    assert_eq!(
        document.cells()[4],
        format!("Niezwłocznie · najpóźniej {date}")
    );
    document.is_overdue = true;
    assert_eq!(document.cells()[4], format!("PO TERMINIE · {date}"));
}

fn document() -> Document {
    Document {
        kind: ReceiptKind::Sale,
        order_id: uuid::Uuid::nil(),
        order_number: 1,
        allocation_id: None,
        amount: 100,
        customer_name: None,
        occurred_at: Timestamp::UNIX_EPOCH,
        due_at: Timestamp::UNIX_EPOCH,
        latest_at: Timestamp::UNIX_EPOCH,
        is_overdue: false,
    }
}

#[test]
fn document_columns_sort_raw_numbers_and_instants_in_both_directions() {
    let documents = [
        Document {
            order_number: 10,
            amount: 1_000,
            customer_name: Some("Żak".into()),
            latest_at: "2025-12-01T00:00:00Z".parse().unwrap(),
            ..document()
        },
        Document {
            kind: ReceiptKind::Advance,
            order_number: 2,
            amount: 900,
            customer_name: None,
            latest_at: "2026-01-01T00:00:00Z".parse().unwrap(),
            ..document()
        },
        Document {
            order_number: 3,
            amount: 99,
            customer_name: Some("Adam".into()),
            latest_at: "2025-02-01T00:00:00Z".parse().unwrap(),
            ..document()
        },
    ];
    for (column, ascending, descending) in [
        (0, [2, 3, 10], [10, 3, 2]),
        (1, [10, 3, 2], [2, 10, 3]),
        (2, [2, 3, 10], [10, 3, 2]),
        (3, [3, 2, 10], [10, 2, 3]),
        (4, [3, 10, 2], [2, 10, 3]),
    ] {
        for (descending, expected) in [(false, ascending), (true, descending)] {
            let mut table = Table::new();
            let _previous = table.set(documents.to_vec());
            let _selection = table.sort(Sort { column, descending }, None);
            assert_eq!(
                table
                    .items()
                    .iter()
                    .map(|row| row.order_number)
                    .collect::<Vec<_>>(),
                expected,
                "column {column}, descending {descending}"
            );
        }
    }
}

#[test]
fn history_columns_sort_raw_values_and_lexicographic_item_sequences() {
    let entries = [
        (
            ["Żak"].as_slice(),
            "2025-12-01T00:00:00Z",
            "10",
            1_000,
            PaymentForm::Cash,
        ),
        (
            ["Adam"].as_slice(),
            "2026-01-01T00:00:00Z",
            "2",
            900,
            PaymentForm::Cash,
        ),
        (
            ["Adam", "Łukasz"].as_slice(),
            "2025-02-01T00:00:00Z",
            "3",
            99,
            PaymentForm::Card,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (items, printed_at, number, amount, form))| {
        let total = Grosze::new(amount).unwrap();
        HistoryEntry {
            job_id: uuid::Uuid::from_u128(u128::try_from(index).unwrap()),
            printed_at: printed_at.parse().unwrap(),
            receipt_number: Some(number.into()),
            total,
            payments: vec![(form, total)],
            items: items.iter().map(|item| (*item).into()).collect(),
        }
    })
    .collect::<Vec<_>>();
    for (column, ascending) in [
        (0, [1, 2, 0]),
        (1, [2, 0, 1]),
        (2, [1, 2, 0]),
        (3, [2, 1, 0]),
        (4, [1, 0, 2]),
    ] {
        for descending in [false, true] {
            let mut expected = ascending.map(uuid::Uuid::from_u128);
            if descending {
                expected.reverse();
            }
            let mut table = Table::new();
            let _previous = table.set(entries.clone());
            let _selection = table.sort(Sort { column, descending }, None);
            assert_eq!(
                table
                    .items()
                    .iter()
                    .map(|row| row.job_id)
                    .collect::<Vec<_>>(),
                expected,
                "column {column}, descending {descending}"
            );
        }
    }
}
