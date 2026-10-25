use std::array;

use agent::documents::ReceiptKind;

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
    let mut names = [
        "Żak", "Źrebak", "Zenon", "Łukasz", "Lz", "ącki", "adam", "Ćma", "Czajka",
    ];
    names.sort_by(|a, b| text_order(a, b));
    assert_eq!(
        names,
        [
            "adam", "ącki", "Czajka", "Ćma", "Lz", "Łukasz", "Zenon", "Źrebak", "Żak"
        ],
        "Polish letters follow their unaccented letter"
    );
}

#[test]
fn receipt_counters_sort_numerically_before_staff_entered_numbers() {
    let mut numbers = [
        Some("staff-7"),
        Some("10"),
        None,
        Some("9"),
        Some("2"),
        Some("02"),
    ];
    numbers.sort_by(|a, b| option_order(*a, *b, receipt_order));
    assert_eq!(
        numbers,
        [
            None,
            Some("02"),
            Some("2"),
            Some("9"),
            Some("10"),
            Some("staff-7")
        ],
        "numeric counters precede manual references"
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
    let mut document = Document {
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
    };
    let earlier = document.clone();
    document.latest_at = "2026-10-24T10:00:00Z".parse().unwrap();
    assert_eq!(document.compare(&earlier, 4), Ordering::Greater);
    // A bank-settled advance before its statutory limit is still due now.
    assert!(document.cells()[4].starts_with("Niezwłocznie · najpóźniej "));
    assert!(document.cells()[4].contains("24.10.2026"));
    document.is_overdue = true;
    assert!(document.cells()[4].starts_with("PO TERMINIE · "));
    assert!(document.cells()[4].contains("24.10.2026"));
}
