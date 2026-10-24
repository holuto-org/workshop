use std::rc::Rc;

use agent::{
    documents::{BuyerNip, Document},
    history::HistoryEntry,
    settings::{BAUD_RATES, Settings},
    worker::{Command, Tone},
};
use jiff::{Span, Timestamp, Zoned, civil::Date, tz::TimeZone};
use posnet::printer::Status;
use rust_decimal::Decimal;
use slint::{ModelRc, VecModel};

use crate::ui::{Action, AppWindow, Date as PickerDate, Tab, TableRow};

#[derive(Debug)]
pub(crate) struct Confirmation {
    command: Command,
    title: String,
    text: String,
    action: String,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "Worker capabilities are independent flags, not mutually exclusive states"
)]
pub(crate) struct Model {
    pub(crate) settings: Settings,
    pub(crate) documents: Vec<Document>,
    history: Vec<HistoryEntry>,
    document_rows: Rc<VecModel<TableRow>>,
    history_rows: Rc<VecModel<TableRow>>,
    pub(crate) selected: Option<usize>,
    pub(crate) page: usize,
    pub(crate) has_next: bool,
    pub(crate) running: bool,
    pub(crate) stopping: bool,
    pub(crate) connected: bool,
    pub(crate) serial: String,
    pub(crate) device: Option<Status>,
    pub(crate) blocked: bool,
    pub(crate) report_unresolved: bool,
    pub(crate) tab: Tab,
    pub(crate) nip: String,
    pub(crate) message: String,
    pub(crate) tone: Tone,
    pub(crate) months: Vec<(i16, i8, String)>,
    pub(crate) month_index: usize,
    pub(crate) from: Date,
    pub(crate) to: Date,
    pub(crate) summary: bool,
    pending: Option<Confirmation>,
}

impl Model {
    pub(crate) fn new(settings: Settings) -> Self {
        let today = Zoned::now().date();
        let months = (1..=24)
            .filter_map(|back| {
                today
                    .first_of_month()
                    .checked_sub(Span::new().months(back))
                    .ok()
            })
            .map(|date| {
                (
                    date.year(),
                    date.month(),
                    format!("{} {}", month_name(date.month()), date.year()),
                )
            })
            .collect();
        Self {
            settings,
            documents: Vec::new(),
            history: Vec::new(),
            document_rows: Rc::new(VecModel::default()),
            history_rows: Rc::new(VecModel::default()),
            selected: None,
            page: 1,
            has_next: false,
            running: false,
            stopping: false,
            connected: false,
            serial: String::new(),
            device: None,
            blocked: false,
            report_unresolved: false,
            tab: Tab::Settings,
            nip: String::new(),
            message: "Zaloguj się tokenem drukarki z Holuto.".into(),
            tone: Tone::Info,
            months,
            month_index: 0,
            from: today.checked_sub(Span::new().days(7)).unwrap_or(today),
            to: today,
            summary: false,
            pending: None,
        }
    }

    pub(crate) fn say(&mut self, text: String, tone: Tone) {
        self.message = text;
        self.tone = tone;
    }

    pub(crate) fn set_documents(&mut self, documents: Vec<Document>, page: usize, has_next: bool) {
        let selected = self.selected.and_then(|index| self.documents.get(index));
        self.selected =
            selected.and_then(|old| documents.iter().position(|new| new.same_document(old)));
        if self.documents != documents {
            self.document_rows
                .set_vec(documents.iter().map(document_row).collect::<Vec<_>>());
        }
        self.documents = documents;
        self.page = page;
        self.has_next = has_next;
    }

    pub(crate) fn set_history(&mut self, history: Vec<HistoryEntry>) {
        if self.history != history {
            self.history_rows
                .set_vec(history.iter().map(history_row).collect::<Vec<_>>());
            self.history = history;
        }
    }

    pub(crate) fn initialize(&self, window: &AppWindow) {
        window.set_baud_rates(strings(BAUD_RATES.iter().map(u32::to_string)));
        window.set_months(strings(self.months.iter().map(|month| month.2.clone())));
        window.set_documents(Rc::clone(&self.document_rows).into());
        window.set_history(Rc::clone(&self.history_rows).into());
        self.sync(window);
    }

    pub(crate) fn capture(&mut self, window: &AppWindow) {
        window
            .get_api_url()
            .trim()
            .clone_into(&mut self.settings.api_url);
        window
            .get_token()
            .trim()
            .clone_into(&mut self.settings.token);
        window.get_port().trim().clone_into(&mut self.settings.port);
        if let Ok(index) = usize::try_from(window.get_baud_index())
            && let Some(baud) = BAUD_RATES.get(index)
        {
            self.settings.baud_rate = *baud;
        }
        self.nip = window.get_nip().to_string();
        self.month_index = usize::try_from(window.get_month_index()).unwrap_or_default();
        self.summary = window.get_summary();
        self.tab = window.get_tab();
    }

    pub(crate) fn sync(&self, window: &AppWindow) {
        window.set_tab(self.tab);
        window.set_running(self.running);
        window.set_stopping(self.stopping);
        window.set_connected(self.connected);
        window.set_queue_enabled(self.connected && self.selected.is_some() && !self.blocked);
        window.set_reports_enabled(self.reports_enabled());
        window.set_previous_enabled(self.connected && self.page > 1);
        window.set_next_enabled(self.connected && self.has_next);
        window.set_report_unresolved(self.report_unresolved);
        window.set_status(self.message.as_str().into());
        window.set_status_tone(tone_number(self.tone));
        window.set_connection_text(self.connection_text().into());
        window.set_connection_tone(
            if self.blocked || self.device.as_ref().is_some_and(|status| !status.ready()) {
                2
            } else {
                0
            },
        );
        window.set_api_url(self.settings.api_url.as_str().into());
        window.set_token(self.settings.token.as_str().into());
        window.set_port(self.settings.port.as_str().into());
        window.set_baud_index(
            BAUD_RATES
                .iter()
                .position(|baud| *baud == self.settings.baud_rate)
                .and_then(|i| i32::try_from(i).ok())
                .unwrap_or_default(),
        );
        window.set_month_index(i32::try_from(self.month_index).unwrap_or_default());
        window.set_nip(self.nip.as_str().into());
        window.set_selected(
            self.selected
                .and_then(|index| i32::try_from(index).ok())
                .unwrap_or(-1),
        );
        window.set_page_label(format!("Strona {}", self.page).into());
        window.set_from_date(ui_date(self.from));
        window.set_to_date(ui_date(self.to));
        window.set_from_label(date_label(self.from).into());
        window.set_to_label(date_label(self.to).into());
        window.set_summary(self.summary);
        window.set_confirming(self.pending.is_some());
        if let Some(pending) = &self.pending {
            window.set_confirm_title(pending.title.as_str().into());
            window.set_confirm_text(pending.text.as_str().into());
            window.set_confirm_action(pending.action.as_str().into());
        }
    }

    fn reports_enabled(&self) -> bool {
        self.connected && !self.blocked && self.device.as_ref().is_some_and(Status::ready)
    }

    fn connection_text(&self) -> String {
        if self.blocked {
            "Zablokowane: wymaga rozstrzygnięcia".into()
        } else if let Some(status) = self.device.as_ref().filter(|status| !status.ready()) {
            format!("Drukarka niegotowa: {}", status.description())
        } else if self.connected {
            format!("Połączono · {}", self.serial)
        } else if self.running {
            "Łączenie…".into()
        } else {
            "Niepołączono".into()
        }
    }

    pub(crate) fn request(&mut self, action: Action) {
        self.pending = None;
        self.pending = match action {
            Action::QueueReceipt => {
                if self.connected && !self.blocked {
                    self.queue_confirmation()
                } else {
                    None
                }
            },
            Action::DailyReport => self.reports_enabled().then(|| Confirmation {
                command: Command::DailyReport,
                title: "Raport dobowy".into(),
                text: concat!(
                    "Raport dobowy zamknie bieżącą sprzedaż dnia w pamięci fiskalnej. ",
                    "Wydrukować raport?"
                )
                .into(),
                action: "Drukuj".into(),
            }),
            Action::MonthlyReport => self
                .months
                .get(self.month_index)
                .filter(|_| self.reports_enabled())
                .map(|month| Confirmation {
                    command: Command::MonthlyReport {
                        year: month.0,
                        month: month.1,
                    },
                    title: "Raport miesięczny".into(),
                    text: format!("Wydrukować raport miesięczny za {}?", month.2),
                    action: "Drukuj".into(),
                }),
            Action::PeriodicReport => {
                if self.reports_enabled() {
                    self.periodic_confirmation()
                } else {
                    None
                }
            },
            Action::ResolveReport => self.report_unresolved.then(|| Confirmation {
                command: Command::ResolveReport,
                title: "Wyjaśnij raport".into(),
                text: concat!(
                    "Sprawdź fizyczny wydruk i stan drukarki. Czy ustalono wynik raportu? ",
                    "To potwierdzenie nie ponawia wydruku."
                )
                .into(),
                action: "Usuń blokadę".into(),
            }),
            Action::Save
            | Action::Start
            | Action::Stop
            | Action::Quit
            | Action::PreviousPage
            | Action::NextPage => None,
        };
    }

    fn queue_confirmation(&mut self) -> Option<Confirmation> {
        let document = self.selected.and_then(|index| self.documents.get(index))?;
        let text = self.nip.trim();
        let buyer_nip = if text.is_empty() {
            None
        } else if let Ok(nip) = BuyerNip::parse(text) {
            Some(nip)
        } else {
            self.say("NIP nabywcy musi zawierać 10 cyfr.".into(), Tone::Error);
            return None;
        };
        Some(Confirmation {
            command: Command::Queue {
                document: document.clone(),
                buyer_nip,
            },
            title: "Potwierdź paragon".into(),
            text: format!(
                "{} · zlecenie {} · {}\nNIP: {}\nZatwierdzenie wystawi paragon fiskalny.",
                document.kind.label(),
                document.order_number,
                amount(document.amount),
                if text.is_empty() { "—" } else { text }
            ),
            action: "Dodaj do wydruku".into(),
        })
    }

    fn periodic_confirmation(&mut self) -> Option<Confirmation> {
        if self.from > self.to {
            self.say(
                "Data początkowa nie może być późniejsza niż końcowa.".into(),
                Tone::Error,
            );
            return None;
        }
        Some(Confirmation {
            command: Command::PeriodicReport {
                from: self.from,
                to: self.to,
                summary: self.summary,
            },
            title: "Raport okresowy".into(),
            text: format!(
                "Wydrukować raport {} za okres {} – {}?",
                if self.summary {
                    "skrócony"
                } else {
                    "szczegółowy"
                },
                date_label(self.from),
                date_label(self.to)
            ),
            action: "Drukuj".into(),
        })
    }

    pub(crate) fn confirm<F: FnMut(Command)>(&mut self, accepted: bool, mut submit: F) {
        if let Some(pending) = self.pending.take()
            && accepted
        {
            // Readiness may have changed while the confirmation was open.
            let allowed = match &pending.command {
                Command::Queue { .. } => self.connected && !self.blocked,
                Command::DailyReport
                | Command::MonthlyReport { .. }
                | Command::PeriodicReport { .. } => self.reports_enabled(),
                Command::ResolveReport => self.report_unresolved,
                Command::NextPage | Command::PreviousPage => false,
            };
            if allowed {
                submit(pending.command);
            }
        }
    }

    pub(crate) fn change_date(&mut self, from: bool, date: &PickerDate) {
        let parsed = i16::try_from(date.year)
            .ok()
            .zip(i8::try_from(date.month).ok())
            .zip(i8::try_from(date.day).ok())
            .and_then(|((year, month), day)| Date::new(year, month, day).ok());
        if let Some(date) = parsed.filter(|date| *date <= Zoned::now().date()) {
            if from {
                self.from = date;
            } else {
                self.to = date;
            }
        } else {
            self.say(
                "Data nie może być późniejsza niż dzisiejsza.".into(),
                Tone::Error,
            );
        }
    }
}

fn ui_date(date: Date) -> PickerDate {
    PickerDate {
        day: i32::from(date.day()),
        month: i32::from(date.month()),
        year: i32::from(date.year()),
    }
}
const fn tone_number(tone: Tone) -> i32 {
    match tone {
        Tone::Info => 0,
        Tone::Success => 1,
        Tone::Error => 2,
    }
}
fn date_label(date: Date) -> String {
    date.strftime("%d.%m.%Y").to_string()
}
fn timestamp_label(timestamp: Timestamp) -> String {
    timestamp
        .to_zoned(TimeZone::system())
        .strftime("%d.%m.%Y %H:%M")
        .to_string()
}
fn amount(value: Decimal) -> String {
    format!("{value:.2} zł").replace('.', ",")
}
pub(crate) fn strings<I: IntoIterator<Item = String>>(values: I) -> ModelRc<slint::SharedString> {
    Rc::new(VecModel::from(
        values.into_iter().map(Into::into).collect::<Vec<_>>(),
    ))
    .into()
}
fn document_row(document: &Document) -> TableRow {
    TableRow {
        a: document.order_number.to_string().into(),
        b: document.kind.label().into(),
        c: document.customer_name.as_deref().unwrap_or("—").into(),
        d: amount(document.amount).into(),
        e: timestamp_label(document.occurred_at).into(),
    }
}
fn history_row(entry: &HistoryEntry) -> TableRow {
    TableRow {
        a: entry.items.join(", ").into(),
        b: timestamp_label(entry.printed_at).into(),
        c: entry.receipt_number.as_deref().unwrap_or("—").into(),
        d: entry.total.to_string().into(),
        e: if entry.payments.is_empty() {
            "—".into()
        } else {
            entry
                .payments
                .iter()
                .map(|(form, amount)| format!("{}: {amount}", form.label()))
                .collect::<Vec<_>>()
                .join(", ")
                .into()
        },
    }
}
const fn month_name(month: i8) -> &'static str {
    match month {
        1 => "styczeń",
        2 => "luty",
        3 => "marzec",
        4 => "kwiecień",
        5 => "maj",
        6 => "czerwiec",
        7 => "lipiec",
        8 => "sierpień",
        9 => "wrzesień",
        10 => "październik",
        11 => "listopad",
        12 => "grudzień",
        _ => "",
    }
}

#[cfg(test)]
mod tests;
