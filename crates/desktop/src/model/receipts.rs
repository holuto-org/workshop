//! Server-paged receipt history; presentation never filters or merges local fiscal evidence.
use std::{fmt::Write as _, rc::Rc};

use agent::api::model::{JobSource, JobState, JobView};
use fiscal::money::Grosze;
use jiff::{civil::Date, tz::TimeZone};
use slint::{Model as _, ModelRc, VecModel};
use uuid::Uuid;

use super::table::timestamp_label;
use crate::ui::ReceiptRow;

pub(super) struct Receipts {
    items: Vec<JobView>,
    selected: Option<Uuid>,
    rows: Rc<VecModel<ReceiptRow>>,
    pub(super) detail: String,
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
            detail: String::new(),
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
            let mut summary = format!(
                "{} · {}\n{} · {}\n{} · {}",
                job.kind.label(),
                amount(&job.total),
                view.customer_name.as_deref().unwrap_or("Bez klienta"),
                view.vehicle_plate.as_deref().unwrap_or("Bez rejestracji"),
                job.receipt_number.as_deref().map_or_else(
                    || "Brak numeru paragonu".into(),
                    |number| format!("Nr {number}")
                ),
                job.issued_at
                    .map_or_else(|| "Brak daty wystawienia".into(), timestamp_label)
            );
            if let Some(result) = &job.result {
                summary.push('\n');
                summary.push_str(result);
            }
            let row = ReceiptRow {
                group: group.into(),
                order: format!("Zlecenie {}", job.order_number).into(),
                status: status(job.state).into(),
                summary: summary.into(),
                tone: match job.state {
                    JobState::Done => 1,
                    JobState::Failed | JobState::Expired | JobState::Unknown | JobState::Voided => {
                        2
                    },
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
            .map_or_else(String::new, detail);
    }
}

fn detail(view: &JobView) -> String {
    let job = &view.job;
    let title = job
        .receipt_number
        .as_deref()
        .map_or_else(|| "Paragon".into(), |number| format!("Paragon {number}"));
    let mut text = format!(
        concat!(
            "{}\n{} · {}\n\nZlecenie {}\nKlient: {}\nRejestracja: {}\n",
            "Rodzaj: {}\nWystawiono: {}\nŹródło: {}\nNIP nabywcy: {}\nPłatność: {}\n\n{}"
        ),
        status(job.state),
        title,
        amount(&job.total),
        job.order_number,
        view.customer_name.as_deref().unwrap_or("Bez klienta"),
        view.vehicle_plate.as_deref().unwrap_or("Bez rejestracji"),
        job.kind.label(),
        job.issued_at.map_or_else(|| "—".into(), timestamp_label),
        match view.source {
            JobSource::Staff => "Holuto",
            JobSource::Device => "Urządzenie",
        },
        job.buyer_nip.as_deref().unwrap_or("Bez NIP"),
        job.payments
            .iter()
            .map(|payment| format!("{}: {}", payment.form.label(), amount(&payment.amount)))
            .collect::<Vec<_>>()
            .join(", "),
        job.lines
            .iter()
            .map(|line| format!(
                "{} · {} · {}",
                line.name,
                line.quantity,
                amount(&line.total)
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
    if !job.advances.is_empty() {
        text.push_str("\n\nOdliczone zaliczki:");
        for advance in &job.advances {
            let _written = write!(
                text,
                "\nVAT {} · {}",
                advance.vat_rate.label(),
                amount(&advance.amount)
            );
        }
    }
    if let Some(result) = &job.result {
        text.push_str("\n\nWynik: ");
        text.push_str(result);
    }
    if job.state == JobState::Unknown {
        text.push_str("\n\nWynik niepewny — sprawdź wydruk i rozstrzygnij zadanie w Holuto.");
    }
    text
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
fn day_label(day: Date, today: Date) -> String {
    if day == today {
        format!("Dziś · {}", day.strftime("%d.%m"))
    } else if day == today.yesterday().unwrap_or(today) {
        format!("Wczoraj · {}", day.strftime("%d.%m"))
    } else {
        day.strftime("%d.%m.%Y").to_string()
    }
}
