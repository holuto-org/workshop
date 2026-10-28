//! Server-paged receipt history; presentation never filters or merges local fiscal evidence.
//! Cards and details show a field only when it has a value, and each fact only once.
use std::rc::Rc;

use agent::{
    api::model::{JobState, JobView},
    documents::ReceiptKind,
};
use fiscal::money::Grosze;
use jiff::{
    Timestamp,
    civil::{Date, Weekday},
    tz::TimeZone,
};
use slint::{Model as _, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use crate::ui::{DetailLine, DetailTotal, ReceiptDetail, ReceiptRow};

pub(super) struct Receipts {
    items: Vec<JobView>,
    selected: Option<Uuid>,
    rows: Rc<VecModel<ReceiptRow>>,
    pub(super) detail: ReceiptDetail,
    pub(super) page: usize,
    pub(super) has_previous: bool,
    pub(super) has_next: bool,
    pub(super) unavailable: bool,
}
impl Receipts {
    pub(super) fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: None,
            rows: Rc::new(VecModel::default()),
            detail: ReceiptDetail::default(),
            page: 1,
            has_previous: false,
            has_next: false,
            unavailable: false,
        }
    }

    pub(super) fn set(
        &mut self,
        items: Vec<JobView>,
        page: usize,
        has_previous: bool,
        has_next: bool,
        today: Date,
    ) {
        self.items = items;
        self.page = page;
        self.has_previous = has_previous;
        self.has_next = has_next;
        self.unavailable = false;
        self.refresh(today);
    }

    pub(super) fn unavailable(&mut self, today: Date) {
        self.set(Vec::new(), 1, false, false, today);
        self.unavailable = true;
    }

    pub(super) fn rows(&self) -> ModelRc<ReceiptRow> {
        Rc::clone(&self.rows).into()
    }

    pub(super) fn select(&mut self, index: i32) {
        self.selected = usize::try_from(index)
            .ok()
            .and_then(|index| self.items.get(index))
            .map(|view| view.job.id);
        self.refresh_detail();
    }

    pub(super) fn index(&self) -> i32 {
        self.selected
            .and_then(|id| self.items.iter().position(|view| view.job.id == id))
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1)
    }

    pub(super) fn refresh(&mut self, today: Date) {
        let mut rows = Vec::with_capacity(self.items.len());
        let mut previous_day = None;
        for view in &self.items {
            let job = &view.job;
            let day = job.created_at.to_zoned(TimeZone::system()).date();
            let group = if previous_day == Some(day) {
                String::new()
            } else {
                day_label(day, today)
            };
            previous_day = Some(day);
            let row = ReceiptRow {
                group: group.into(),
                order: format!("Zlecenie {}", job.order_number).into(),
                status: status(job.state).into(),
                customer: joined([view.customer_name.as_deref(), view.vehicle_plate.as_deref()]),
                amount: match job.kind {
                    ReceiptKind::Advance => format!("Zaliczka · {}", amount(&job.total)),
                    ReceiptKind::Sale => amount(&job.total),
                }
                .into(),
                receipt: joined([
                    job.receipt_number
                        .as_deref()
                        .map(|number| format!("Nr {number}"))
                        .as_deref(),
                    job.issued_at
                        .map(|issued| issue_label(issued, day))
                        .as_deref(),
                ]),
                note: job.result.as_deref().unwrap_or_default().into(),
                // Red only where the operator must act; finished outcomes stay neutral.
                tone: match job.state {
                    JobState::Done => 1,
                    JobState::Failed | JobState::Unknown => 2,
                    JobState::Expired | JobState::Voided => 3,
                    JobState::Queued | JobState::Taken => 0,
                },
            };
            rows.push(row);
        }
        if self.rows.iter().ne(rows.iter().cloned()) {
            self.rows.set_vec(rows);
        }
        if !self
            .selected
            .is_some_and(|id| self.items.iter().any(|view| view.job.id == id))
        {
            self.selected = self.items.first().map(|view| view.job.id);
        }
        self.refresh_detail();
    }

    fn refresh_detail(&mut self) {
        self.detail = self
            .selected
            .and_then(|id| self.items.iter().find(|view| view.job.id == id))
            .map_or_else(ReceiptDetail::default, detail);
    }
}

/// The selected card already shows the state, amount, receipt number and failure reason.
fn detail(view: &JobView) -> ReceiptDetail {
    let job = &view.job;
    let lines: Vec<DetailLine> = job
        .lines
        .iter()
        .map(|line| DetailLine {
            name: line.name.as_str().into(),
            quantity: line.quantity.as_str().into(),
            price: amount(&line.unit_price).into(),
            total: amount(&line.total).into(),
        })
        .collect();
    let mut totals = vec![DetailTotal {
        label: "Razem".into(),
        amount: amount(&job.total).into(),
    }];
    totals.extend(job.advances.iter().map(|advance| DetailTotal {
        label: format!("Odliczona zaliczka · VAT {}", advance.vat_rate.label()).into(),
        amount: amount(&advance.amount).into(),
    }));
    totals.extend(job.payments.iter().map(|payment| DetailTotal {
        label: format!("Płatność: {}", payment.form.label()).into(),
        amount: amount(&payment.amount).into(),
    }));
    ReceiptDetail {
        order: format!("Zlecenie {}", job.order_number).into(),
        customer: joined([view.customer_name.as_deref(), view.vehicle_plate.as_deref()]),
        buyer_nip: job
            .buyer_nip
            .as_deref()
            .map(|nip| format!("NIP nabywcy {nip}"))
            .unwrap_or_default()
            .into(),
        lines: Rc::new(VecModel::from(lines)).into(),
        totals: Rc::new(VecModel::from(totals)).into(),
        warning: if job.state == JobState::Unknown {
            "Wynik niepewny — sprawdź wydruk i rozstrzygnij zadanie w Holuto.".into()
        } else {
            SharedString::new()
        },
    }
}
/// The present values joined with " · "; empty when none has a value.
fn joined<const N: usize>(parts: [Option<&str>; N]) -> SharedString {
    parts
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
        .into()
}
fn amount(total: &str) -> String {
    Grosze::parse(total).map_or_else(|_| total.to_owned(), |amount| amount.to_string())
}
const fn status(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "W kolejce",
        JobState::Taken => "Drukuje się…",
        JobState::Done => "Wydrukowano",
        JobState::Failed => "Nie wydrukowano",
        JobState::Expired => "Wygasło",
        JobState::Unknown => "Do sprawdzenia",
        JobState::Voided => "Anulowano",
    }
}
/// One shape for every day; the year appears only when it is not the current one.
fn day_label(day: Date, today: Date) -> String {
    if day == today {
        format!("Dziś · {}", day.strftime("%d.%m"))
    } else if day == today.yesterday().unwrap_or(today) {
        format!("Wczoraj · {}", day.strftime("%d.%m"))
    } else if day.year() == today.year() {
        format!("{} · {}", weekday(day.weekday()), day.strftime("%d.%m"))
    } else {
        day.strftime("%d.%m.%Y").to_string()
    }
}
const fn weekday(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Monday => "Poniedziałek",
        Weekday::Tuesday => "Wtorek",
        Weekday::Wednesday => "Środa",
        Weekday::Thursday => "Czwartek",
        Weekday::Friday => "Piątek",
        Weekday::Saturday => "Sobota",
        Weekday::Sunday => "Niedziela",
    }
}
/// The time alone under its own day heading; the full date when the receipt was
/// issued on another day than the job was created (its fiscal date differs).
fn issue_label(issued: Timestamp, group: Date) -> String {
    let issued = issued.to_zoned(TimeZone::system());
    if issued.date() == group {
        issued.strftime("%H:%M").to_string()
    } else {
        issued.strftime("%d.%m.%Y %H:%M").to_string()
    }
}

#[cfg(test)]
#[path = "receipts/tests.rs"]
mod tests;
