//! Rows for the `StandardTableView`s: Slint draws them, Rust owns their order.

use std::{cmp::Ordering, mem, rc::Rc};

use agent::{documents::Document, history::HistoryEntry};
use jiff::{Timestamp, tz::TimeZone};
use posnet::money::{Grosze, zloty};
use slint::{
    Model as _, ModelRc, SharedString, VecModel,
    language::{SortOrder, StandardListViewItem, TableColumn},
};

/// Both tables show five columns; the `.slint` column lists match `Row::cells`.
const COLUMNS: usize = 5;

/// One table row type: its cells and how a column orders it.
pub(crate) trait Row: PartialEq {
    fn cells(&self) -> [SharedString; COLUMNS];
    /// Orders by the underlying values (amounts, timestamps), not by the formatted text.
    fn compare(&self, other: &Self, column: usize) -> Ordering;
}

/// The column the operator sorted by, from a header click.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sort {
    column: usize,
    descending: bool,
}

impl Sort {
    pub(crate) fn new(column: i32, descending: bool) -> Option<Self> {
        usize::try_from(column)
            .ok()
            .filter(|column| *column < COLUMNS)
            .map(|column| Self { column, descending })
    }

    fn compare<T: Row>(self, a: &T, b: &T) -> Ordering {
        let ordering = a.compare(b, self.column);
        if self.descending {
            ordering.reverse()
        } else {
            ordering
        }
    }

    const fn order(self) -> SortOrder {
        if self.descending {
            SortOrder::Descending
        } else {
            SortOrder::Ascending
        }
    }
}

pub(crate) struct Table<T> {
    items: Vec<T>,
    rows: Rc<VecModel<ModelRc<StandardListViewItem>>>,
    sort: Option<Sort>,
}

impl<T: Row> Table<T> {
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            rows: Rc::new(VecModel::default()),
            sort: None,
        }
    }

    pub(crate) fn items(&self) -> &[T] {
        &self.items
    }

    pub(crate) fn rows(&self) -> ModelRc<ModelRc<StandardListViewItem>> {
        Rc::clone(&self.rows).into()
    }

    /// Replaces the items in the operator's order; returns the items they replaced, so a
    /// caller can find its selected record again.
    pub(crate) fn set(&mut self, mut items: Vec<T>) -> Vec<T> {
        if let Some(sort) = self.sort {
            items.sort_by(|a, b| sort.compare(a, b));
        }
        if self.items == items {
            return items;
        }
        self.rows.set_vec(items.iter().map(row).collect::<Vec<_>>());
        mem::replace(&mut self.items, items)
    }

    /// Sorts stably; returns where the row at `selected` moved.
    pub(crate) fn sort(&mut self, sort: Sort, selected: Option<usize>) -> Option<usize> {
        self.sort = Some(sort);
        let mut tagged: Vec<_> = mem::take(&mut self.items).into_iter().enumerate().collect();
        tagged.sort_by(|(_, a), (_, b)| sort.compare(a, b));
        let selected = selected.and_then(|old| tagged.iter().position(|(index, _)| *index == old));
        self.items = tagged.into_iter().map(|(_, item)| item).collect();
        self.rows
            .set_vec(self.items.iter().map(row).collect::<Vec<_>>());
        selected
    }

    /// Marks only the sorted column, also in a window recreated from the tray.
    pub(crate) fn sync_columns(&self, columns: &ModelRc<TableColumn>) {
        for index in 0..columns.row_count() {
            let order = match self.sort {
                Some(sort) if sort.column == index => sort.order(),
                _ => SortOrder::Unsorted,
            };
            if let Some(mut column) = columns.row_data(index)
                && column.sort_order != order
            {
                column.sort_order = order;
                columns.set_row_data(index, column);
            }
        }
    }
}

fn row<T: Row>(item: &T) -> ModelRc<StandardListViewItem> {
    ModelRc::from(item.cells().map(StandardListViewItem::from))
}

impl Row for Document {
    fn cells(&self) -> [SharedString; COLUMNS] {
        [
            self.order_number.to_string().into(),
            self.kind.label().into(),
            self.customer_name.as_deref().unwrap_or("—").into(),
            zloty(self.amount).into(),
            timestamp_label(self.occurred_at).into(),
        ]
    }

    fn compare(&self, other: &Self, column: usize) -> Ordering {
        match column {
            0 => self.order_number.cmp(&other.order_number),
            1 => self.kind.label().cmp(other.kind.label()),
            2 => self.customer_name.cmp(&other.customer_name),
            3 => self.amount.cmp(&other.amount),
            4 => self.occurred_at.cmp(&other.occurred_at),
            _ => Ordering::Equal,
        }
    }
}

impl Row for HistoryEntry {
    fn cells(&self) -> [SharedString; COLUMNS] {
        [
            self.items.join(", ").into(),
            timestamp_label(self.printed_at).into(),
            self.receipt_number.as_deref().unwrap_or("—").into(),
            self.total.to_string().into(),
            if self.payments.is_empty() {
                "—".into()
            } else {
                self.payments
                    .iter()
                    .map(|(form, amount)| format!("{}: {amount}", form.label()))
                    .collect::<Vec<_>>()
                    .join(", ")
                    .into()
            },
        ]
    }

    fn compare(&self, other: &Self, column: usize) -> Ordering {
        match column {
            0 => self.items.cmp(&other.items),
            1 => self.printed_at.cmp(&other.printed_at),
            2 => self.receipt_number.cmp(&other.receipt_number),
            3 => self.total.cmp(&other.total),
            4 => payments(self).cmp(payments(other)),
            _ => Ordering::Equal,
        }
    }
}

fn payments(entry: &HistoryEntry) -> impl Iterator<Item = (&'static str, Grosze)> {
    entry
        .payments
        .iter()
        .map(|(form, amount)| (form.label(), *amount))
}

fn timestamp_label(timestamp: Timestamp) -> String {
    timestamp
        .to_zoned(TimeZone::system())
        .strftime("%d.%m.%Y %H:%M")
        .to_string()
}
