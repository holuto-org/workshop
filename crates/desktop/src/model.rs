#[path = "model/receipts.rs"]
mod receipts;
#[path = "model/table.rs"]
mod table;

use agent::{
    api::model::JobView,
    documents::{BuyerNip, Document, DocumentSearch},
    settings::{BAUD_RATES, Settings, Token},
    status::Snapshot,
    worker::{Command, Searches, Tone},
};
use fiscal::{
    device::{Device, Vendor},
    money::zloty,
};
use jiff::{Timestamp, ToSpan, civil::Date};
use slint::{ModelRc, SharedString, VecModel};
use table::{Sort, Table};

use crate::ui::{Action, AppWindow};

/// Must match `AppWindow`'s tab order.
#[derive(Clone, Copy)]
pub(crate) enum Tab {
    Receipts,
    Reports,
    Settings,
}

impl Tab {
    const fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Receipts),
            1 => Some(Self::Reports),
            2 => Some(Self::Settings),
            _ => None,
        }
    }

    const fn index(self) -> i32 {
        match self {
            Self::Receipts => 0,
            Self::Reports => 1,
            Self::Settings => 2,
        }
    }
}

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
    /// Persisted API address and pairing; address edits are judged against these,
    /// not an earlier capture, so reverting an edit keeps the pairing.
    saved_api_url: String,
    saved_token: Token,
    pub(crate) pairing_code: String,
    pub(crate) pairing: bool,
    documents: Table<Document>,
    receipts: receipts::Receipts,
    receipt_search: String,
    applied_receipt_search: Option<DocumentSearch>,
    pub(crate) manual: bool,
    pub(crate) selected: Option<usize>,
    pub(crate) page: usize,
    pub(crate) has_previous: bool,
    pub(crate) has_next: bool,
    pub(crate) running: bool,
    pub(crate) stopping: bool,
    pub(crate) saving: bool,
    pub(crate) connected: bool,
    pub(crate) unique_number: String,
    pub(crate) vendor: Option<Vendor>,
    pub(crate) printer_id: Option<String>,
    pub(crate) device: Option<Device>,
    pub(crate) blocked: bool,
    pub(crate) report_unresolved: bool,
    /// Evidence the acknowledgement quotes; never the transient status line.
    pub(crate) orphaned_receipt: Option<String>,
    pub(crate) tab: Tab,
    pub(crate) nip: String,
    /// The open receipt confirmation rejected the typed NIP.
    nip_invalid: bool,
    /// Typed search text; the worker lists `applied_search`.
    search: String,
    applied_search: Option<DocumentSearch>,
    pub(crate) message: String,
    pub(crate) tone: Tone,
    /// Month starts, newest first.
    pub(crate) months: Vec<Date>,
    pub(crate) month_index: usize,
    /// Raw `dd.mm.rrrr` range; validated on request.
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) summary: bool,
    pending: Option<Confirmation>,
    today: Date,
    in_flight: bool,
}

impl Model {
    pub(crate) fn new(settings: Settings, today: Date) -> Self {
        let months = months_before(today);
        let saved_api_url = settings.api_url.clone();
        let saved_token = settings.token.clone();
        Self {
            settings,
            saved_api_url,
            saved_token,
            pairing_code: String::new(),
            pairing: false,
            documents: Table::new(),
            receipts: receipts::Receipts::new(),
            receipt_search: String::new(),
            applied_receipt_search: None,
            manual: false,
            selected: None,
            page: 1,
            has_previous: false,
            has_next: false,
            running: false,
            stopping: false,
            saving: false,
            connected: false,
            unique_number: String::new(),
            vendor: None,
            printer_id: None,
            device: None,
            blocked: false,
            report_unresolved: false,
            orphaned_receipt: None,
            tab: Tab::Receipts,
            nip: String::new(),
            nip_invalid: false,
            search: String::new(),
            applied_search: None,
            message: String::new(),
            tone: Tone::Info,
            months,
            month_index: 0,
            from: date_label(today.checked_sub(7.days()).unwrap_or(today)),
            to: date_label(today),
            summary: false,
            pending: None,
            today,
            in_flight: false,
        }
    }

    pub(crate) fn say(&mut self, text: String, tone: Tone) {
        self.message = text;
        self.tone = tone;
    }

    pub(crate) fn settings_persisted(&mut self) {
        self.saved_api_url.clone_from(&self.settings.api_url);
        self.saved_token.clone_from(&self.settings.token);
    }

    pub(crate) fn documents(&self) -> &[Document] {
        self.documents.items()
    }

    pub(crate) fn set_documents(
        &mut self,
        documents: Vec<Document>,
        page: usize,
        has_previous: bool,
        has_next: bool,
    ) {
        let previous = self.documents.set(documents);
        self.selected = self
            .selected
            .and_then(|index| previous.get(index))
            .and_then(|old| {
                self.documents
                    .items()
                    .iter()
                    .position(|new| new.same_document(old))
            });
        self.page = page;
        self.has_previous = has_previous;
        self.has_next = has_next;
    }

    pub(crate) fn set_jobs(
        &mut self,
        items: Vec<JobView>,
        page: usize,
        has_previous: bool,
        has_next: bool,
    ) {
        self.receipts
            .set(items, page, has_previous, has_next, self.today);
    }

    pub(crate) fn jobs_unavailable(&mut self) {
        self.receipts.unavailable(self.today);
    }

    pub(crate) const fn jobs_previous_enabled(&self) -> bool {
        self.connected && !self.receipts.unavailable && self.receipts.has_previous
    }

    pub(crate) const fn jobs_next_enabled(&self) -> bool {
        self.connected && !self.receipts.unavailable && self.receipts.has_next
    }

    pub(crate) fn select_receipt(&mut self, index: i32) {
        self.receipts.select(index);
    }

    pub(crate) fn stopped(&mut self) {
        self.running = false;
        self.stopping = false;
        self.connected = false;
        self.in_flight = false;
        // Recheck readiness on reconnect; retain the persisted fiscal block.
        self.device = None;
        // A new worker pages from the start; never offer a stopped session's rows.
        self.set_documents(Vec::new(), 1, false, false);
        self.receipts.set(Vec::new(), 1, false, false, self.today);
    }

    pub(crate) fn initial_searches(&mut self) -> Searches {
        let _documents = self.search_command(false);
        let _receipts = self.search_command(true);
        Searches {
            documents: self.applied_search.clone(),
            jobs: self.applied_receipt_search.clone(),
        }
    }

    /// The worker command listing the typed document or receipt search, unless it is already
    /// listed; invalid text is reported and keeps the listed search.
    pub(crate) fn search_command(&mut self, receipts: bool) -> Option<Command> {
        let (text, applied) = if receipts {
            (&self.receipt_search, &mut self.applied_receipt_search)
        } else {
            (&self.search, &mut self.applied_search)
        };
        match DocumentSearch::parse(text) {
            Ok(search) if search == *applied => None,
            Ok(search) => {
                applied.clone_from(&search);
                Some(if receipts {
                    Command::JobsSearch(search)
                } else {
                    Command::Search(search)
                })
            },
            Err(error) => {
                self.say(error.to_string(), Tone::Error);
                None
            },
        }
    }

    pub(crate) fn status(&self, updated_at: Timestamp) -> Snapshot {
        Snapshot {
            processing: self.running,
            connected: self.connected,
            vendor: self.vendor,
            // Preserve the last connected identity after Stop.
            unique_number: (!self.unique_number.is_empty()).then(|| self.unique_number.clone()),
            printer_id: self.printer_id.clone(),
            blocked: self.blocked,
            report_unresolved: self.report_unresolved,
            orphaned_receipt: self.orphaned_receipt.is_some(),
            updated_at,
        }
    }

    pub(crate) fn status_matches(&self, snapshot: &Snapshot) -> bool {
        let processing = self.running;
        snapshot.processing == processing
            && snapshot.connected == self.connected
            && snapshot.vendor == self.vendor
            && snapshot.unique_number.as_deref()
                == (!self.unique_number.is_empty()).then_some(self.unique_number.as_str())
            && snapshot.printer_id == self.printer_id
            && snapshot.blocked == self.blocked
            && snapshot.report_unresolved == self.report_unresolved
            && snapshot.orphaned_receipt == self.orphaned_receipt.is_some()
    }

    pub(crate) const fn command_completed(&mut self) {
        self.in_flight = false;
    }

    fn roll_months(&mut self, today: Date) -> bool {
        let newest = today.first_of_month().checked_sub(1.month()).ok();
        if self.months.first().copied() == newest {
            return false;
        }
        let months = months_before(today);
        let chosen = self.months.get(self.month_index);
        self.month_index = chosen
            .and_then(|chosen| months.iter().position(|month| month == chosen))
            .unwrap_or_default();
        self.months = months;
        true
    }

    pub(crate) fn sort_documents(&mut self, column: i32, descending: bool) {
        if let Some(sort) = Sort::new(column, descending) {
            self.selected = self.documents.sort(sort, self.selected);
        }
    }

    pub(crate) fn initialize(&mut self, window: &AppWindow, today: Date) {
        let _rolled = self.roll_months(today);
        window.set_baud_rates(strings(BAUD_RATES.iter().map(u32::to_string)));
        window.set_months(strings(self.months.iter().copied().map(month_label)));
        window.set_documents(self.documents.rows());
        self.sync(window);
    }

    pub(crate) fn capture(&mut self, window: &AppWindow, today: Date) {
        let day_changed = self.today != today;
        self.today = today;
        // Slint 1.18 permits assistive edits on disabled controls; guard settings here too.
        if !self.running && !self.saving && !self.pairing {
            self.settings.api_url.clone_from(&self.saved_api_url);
            self.settings.token.clone_from(&self.saved_token);
            self.settings.set_api_url(window.get_api_url().trim());
            window
                .get_pairing_code()
                .trim()
                .clone_into(&mut self.pairing_code);
            window.get_port().trim().clone_into(&mut self.settings.port);
            if let Ok(index) = usize::try_from(window.get_baud_index())
                && let Some(baud) = BAUD_RATES.get(index)
            {
                self.settings.baud_rate = *baud;
            }
        }
        self.settings.telemetry_enabled = window.get_telemetry_enabled();
        self.nip = window.get_nip().to_string();
        self.search = window.get_search().to_string();
        self.receipt_search = window.get_receipt_search().to_string();
        if day_changed {
            self.receipts.refresh(today);
        }
        self.month_index = usize::try_from(window.get_month_index()).unwrap_or_default();
        // Tray sessions can span months.
        if self.roll_months(today) {
            window.set_months(strings(self.months.iter().copied().map(month_label)));
        }
        self.summary = window.get_summary();
        window.get_from_date().trim().clone_into(&mut self.from);
        window.get_to_date().trim().clone_into(&mut self.to);
        if let Some(tab) = Tab::from_index(window.get_tab()) {
            self.tab = tab;
        }
    }

    pub(crate) fn sync(&self, window: &AppWindow) {
        window.set_tab(self.tab.index());
        window.set_running(self.running);
        window.set_stopping(self.stopping);
        window.set_saving_settings(self.saving);
        window.set_pairing(self.pairing);
        window.set_connected(self.connected);
        window.set_queue_enabled(self.queue_enabled() && self.selected.is_some());
        window.set_reports_enabled(self.reports_enabled());
        window.set_daily_report_enabled(self.daily_report_enabled());
        window.set_previous_enabled(self.connected && self.has_previous);
        window.set_next_enabled(self.connected && self.has_next);
        window.set_report_unresolved(self.resolve_enabled());
        window.set_orphaned_receipt(self.acknowledge_enabled());
        window.set_status(self.message.as_str().into());
        window.set_status_tone(tone_number(self.tone));
        window.set_holuto_text(
            if self.connected {
                "Połączono"
            } else if self.running {
                "Łączenie…"
            } else {
                "Nie połączono"
            }
            .into(),
        );
        window.set_printer_text(self.printer_text().into());
        window.set_printer_tone(
            if self.blocked || self.device.as_ref().is_some_and(|status| !status.ready()) {
                2
            } else {
                0
            },
        );
        window.set_receipt_search(self.receipt_search.as_str().into());
        window.set_jobs_unavailable(self.receipts.unavailable);
        window.set_jobs_previous_enabled(self.jobs_previous_enabled());
        window.set_jobs_next_enabled(self.jobs_next_enabled());
        window.set_jobs_page_label(format!("Strona {}", self.receipts.page).into());
        window.set_manual(self.manual);
        window.set_receipts(self.receipts.rows());
        window.set_receipt_selected(self.receipts.index());
        window.set_receipt_detail(self.receipts.detail.as_str().into());
        window.set_api_url(self.settings.api_url.as_str().into());
        window.set_pairing_code(self.pairing_code.as_str().into());
        window.set_start_on_launch(self.settings.start_on_launch);
        window.set_telemetry_enabled(self.settings.telemetry_enabled);
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
        window.set_search(self.search.as_str().into());
        window.set_searching(self.running && self.applied_search.is_some());
        window.set_selected(
            self.selected
                .and_then(|index| i32::try_from(index).ok())
                .unwrap_or(-1),
        );
        window.set_page_label(format!("Strona {}", self.page).into());
        window.set_from_date(self.from.as_str().into());
        window.set_to_date(self.to.as_str().into());
        window.set_summary(self.summary);
        window.set_confirming(self.pending.is_some());
        window.set_confirm_nip(
            self.pending
                .as_ref()
                .is_some_and(|pending| matches!(pending.command, Command::Queue { .. })),
        );
        window.set_nip_invalid(self.nip_invalid);
        if let Some(pending) = &self.pending {
            window.set_confirm_title(pending.title.as_str().into());
            window.set_confirm_text(pending.text.as_str().into());
            window.set_confirm_action(pending.action.as_str().into());
        }
        self.documents.sync_columns(&window.get_document_columns());
    }

    const fn accepting(&self) -> bool {
        self.connected && !self.stopping && !self.in_flight
    }

    const fn queue_enabled(&self) -> bool {
        self.accepting() && !self.blocked
    }

    fn reports_enabled(&self) -> bool {
        self.queue_enabled()
            && self
                .device
                .as_ref()
                .is_some_and(Device::periodic_reports_allowed)
    }

    fn daily_report_enabled(&self) -> bool {
        self.queue_enabled()
            && self
                .device
                .as_ref()
                .is_some_and(Device::daily_report_allowed)
    }

    const fn resolve_enabled(&self) -> bool {
        self.accepting() && self.report_unresolved
    }

    const fn acknowledge_enabled(&self) -> bool {
        self.accepting() && self.orphaned_receipt.is_some()
    }

    fn printer_text(&self) -> String {
        if self.blocked {
            "Zablokowana: wymaga rozstrzygnięcia".into()
        } else if let Some(status) = self.device.as_ref().filter(|status| !status.ready()) {
            format!("Niegotowa: {}", status.description())
        } else if self.connected && self.device.is_some() {
            let vendor = match self.vendor {
                Some(Vendor::Posnet) => "Posnet",
                Some(Vendor::Novitus) => "Novitus",
                Some(Vendor::Elzab) => "Elzab",
                None => "",
            };
            format!("Gotowa · {vendor} {}", self.unique_number)
        } else {
            "Niegotowa: brak połączenia".into()
        }
    }

    pub(crate) fn request(&mut self, action: Action) {
        // Keys and assistive actions still reach controls behind the open dialog.
        if self.pending.is_some() {
            return;
        }
        self.pending = match action {
            Action::QueueReceipt => {
                if self.queue_enabled() {
                    self.queue_confirmation()
                } else {
                    None
                }
            },
            Action::DailyReport => self.daily_report_enabled().then(|| Confirmation {
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
                .map(|&month| Confirmation {
                    command: Command::MonthlyReport {
                        year: month.year(),
                        month: month.month(),
                    },
                    title: "Raport miesięczny".into(),
                    text: format!("Wydrukować raport miesięczny za {}?", month_label(month)),
                    action: "Drukuj".into(),
                }),
            Action::PeriodicReport => {
                if self.reports_enabled() {
                    self.periodic_confirmation()
                } else {
                    None
                }
            },
            Action::ResolveReport => self.resolve_enabled().then(|| Confirmation {
                command: Command::ResolveReport,
                title: "Wyjaśnij raport".into(),
                text: concat!(
                    "Sprawdź fizyczny wydruk i stan drukarki. Czy ustalono wynik raportu? ",
                    "To potwierdzenie nie ponawia wydruku."
                )
                .into(),
                action: "Usuń blokadę".into(),
            }),
            Action::AcknowledgeOrphanedReceipt => {
                let evidence = self.orphaned_receipt.as_deref().unwrap_or_default();
                self.acknowledge_enabled().then(|| Confirmation {
                    command: Command::AcknowledgeOrphanedReceipt,
                    title: "Wyjaśnij paragon".into(),
                    text: format!(
                        concat!(
                            "{}\n\nSprawdź wydruk i uzgodnij dokument w Holuto. ",
                            "Potwierdzenie zapisze lokalne wyjaśnienie i usunie blokadę. ",
                            "Nie wydrukuje paragonu ponownie ani nie zmieni wyniku w Holuto."
                        ),
                        evidence
                    ),
                    action: "Potwierdź wyjaśnienie".into(),
                })
            },
            Action::Save
            | Action::Pair
            | Action::Start
            | Action::Stop
            | Action::Quit
            | Action::Search
            | Action::PreviousPage
            | Action::NextPage
            | Action::JobsSearch
            | Action::JobsFirstPage
            | Action::JobsPreviousPage
            | Action::OpenManual
            | Action::CloseManual
            | Action::JobsNextPage => None,
        };
    }

    fn queue_confirmation(&mut self) -> Option<Confirmation> {
        let document = self
            .selected
            .and_then(|index| self.documents.items().get(index))?;
        let confirmation = Confirmation {
            command: Command::Queue {
                document: document.clone(),
                buyer_nip: None,
            },
            title: "Potwierdź paragon".into(),
            text: format!(
                "{} · zlecenie {} · {}\nZatwierdzenie wystawi paragon fiskalny.",
                document.kind.label(),
                document.order_number,
                zloty(document.amount)
            ),
            action: "Dodaj do wydruku".into(),
        };
        // The buyer NIP belongs to this receipt only.
        self.nip.clear();
        self.nip_invalid = false;
        Some(confirmation)
    }

    fn periodic_confirmation(&mut self) -> Option<Confirmation> {
        let (from, to) = match report_range(&self.from, &self.to, self.today) {
            Ok(range) => range,
            Err(error) => {
                self.say(error, Tone::Error);
                return None;
            },
        };
        Some(Confirmation {
            command: Command::PeriodicReport {
                from,
                to,
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
                date_label(from),
                date_label(to)
            ),
            action: "Drukuj".into(),
        })
    }

    pub(crate) fn confirm(&mut self, accepted: bool) -> Option<Command> {
        if accepted
            && let Some(Command::Queue { buyer_nip, .. }) =
                self.pending.as_mut().map(|pending| &mut pending.command)
        {
            let text = self.nip.trim();
            let parsed = (!text.is_empty()).then(|| BuyerNip::parse(text).ok());
            // An invalid NIP keeps the dialog open so the operator can correct it.
            self.nip_invalid = matches!(parsed, Some(None));
            if self.nip_invalid {
                return None;
            }
            *buyer_nip = parsed.flatten();
        }
        let pending = self.pending.take().filter(|_| accepted)?;
        // The document list may have been refreshed while the confirmation was open.
        if let Command::Queue { document, .. } = &pending.command
            && !self
                .documents()
                .iter()
                .any(|listed| listed.same_document(document))
        {
            self.say(
                "Nie wysłano: dokument nie czeka już na paragon.".into(),
                Tone::Error,
            );
            return None;
        }
        // Readiness may have changed while the confirmation was open.
        let allowed = match &pending.command {
            Command::Queue { .. } => self.queue_enabled(),
            Command::DailyReport => self.daily_report_enabled(),
            Command::MonthlyReport { .. } | Command::PeriodicReport { .. } => {
                self.reports_enabled()
            },
            Command::ResolveReport => self.resolve_enabled(),
            Command::AcknowledgeOrphanedReceipt => self.acknowledge_enabled(),
            Command::NextPage
            | Command::PreviousPage
            | Command::Search(_)
            | Command::JobsFirstPage
            | Command::JobsNextPage
            | Command::JobsPreviousPage
            | Command::JobsSearch(_) => false,
        };
        if !allowed {
            // A closed dialog must not read as a printed receipt or report.
            self.say(
                "Nie wysłano: stan drukarki zmienił się przed potwierdzeniem.".into(),
                Tone::Error,
            );
            return None;
        }
        if matches!(pending.command, Command::Queue { .. }) {
            self.nip.clear();
            self.manual = false;
        }
        self.in_flight = true;
        Some(pending.command)
    }
}

fn report_range(from: &str, to: &str, today: Date) -> Result<(Date, Date), String> {
    let date = |which, text: &str| {
        let padded = text.len() == 10
            && text.bytes().enumerate().all(|(index, byte)| {
                if index == 2 || index == 5 {
                    byte == b'.'
                } else {
                    byte.is_ascii_digit()
                }
            });
        let date = Date::strptime("%d.%m.%Y", text)
            .ok()
            .filter(|_| padded)
            .ok_or_else(|| format!("Data {which} musi mieć postać dd.mm.rrrr."))?;
        if date < Date::constant(2000, 1, 1) {
            return Err(format!(
                "Data {which} nie może być wcześniejsza niż 01.01.2000."
            ));
        }
        if date > today {
            return Err(format!(
                "Data {which} nie może być późniejsza niż dzisiejsza."
            ));
        }
        Ok(date)
    };
    let from = date("początkowa", from)?;
    let to = date("końcowa", to)?;
    if from > to {
        return Err("Data początkowa nie może być późniejsza niż końcowa.".to_owned());
    }
    Ok((from, to))
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
fn months_before(today: Date) -> Vec<Date> {
    today
        .first_of_month()
        .series((-1).month())
        .skip(1)
        .take(24)
        .collect()
}
pub(crate) fn strings<I: IntoIterator<Item = String>>(values: I) -> ModelRc<SharedString> {
    ModelRc::new(
        values
            .into_iter()
            .map(SharedString::from)
            .collect::<VecModel<_>>(),
    )
}
fn month_label(month: Date) -> String {
    format!("{} {}", month_name(month.month()), month.year())
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
#[path = "model/tests.rs"]
mod tests;
