use agent::{
    settings::{BAUD_RATES, Settings},
    status::Snapshot,
    worker::Command,
};
use fiscal::device::{Device, Vendor};
use jiff::{Timestamp, ToSpan, civil::Date};
use slint::{ModelRc, SharedString, VecModel};

use crate::ui::{Action, AppWindow};

/// Must match `AppWindow`'s tab order.
#[derive(Clone, Copy)]
pub(crate) enum Tab {
    Status,
    Reports,
    Settings,
}

impl Tab {
    const fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Status),
            1 => Some(Self::Reports),
            2 => Some(Self::Settings),
            _ => None,
        }
    }

    const fn index(self) -> i32 {
        match self {
            Self::Status => 0,
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
    pub(crate) pairing_code: String,
    pub(crate) pairing: bool,
    /// Start was requested and the worker is not spawned yet.
    pub(crate) starting: bool,
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
    pub(crate) message: String,
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
        Self {
            settings,
            pairing_code: String::new(),
            pairing: false,
            starting: false,
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
            tab: Tab::Status,
            message: String::new(),
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

    pub(crate) fn problem(&mut self, text: String) {
        self.message = text;
    }

    pub(crate) fn clear_problem(&mut self) {
        self.message.clear();
    }

    pub(crate) fn stopped(&mut self) {
        self.running = false;
        self.stopping = false;
        self.connected = false;
        self.in_flight = false;
        // Recheck readiness on reconnect; retain the persisted fiscal block.
        self.device = None;
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

    pub(crate) fn initialize(&mut self, window: &AppWindow, today: Date) {
        let _rolled = self.roll_months(today);
        window.set_baud_rates(strings(BAUD_RATES.iter().map(u32::to_string)));
        window.set_months(strings(self.months.iter().copied().map(month_label)));
        self.sync(window);
    }

    pub(crate) fn capture(&mut self, window: &AppWindow, today: Date) {
        self.today = today;
        // Slint 1.18 permits assistive edits on disabled controls; guard settings here too.
        if !self.running && !self.saving && !self.pairing {
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
        window.set_starting(self.starting);
        window.set_reports_enabled(self.reports_enabled());
        window.set_daily_report_enabled(self.daily_report_enabled());
        window.set_report_unresolved(self.resolve_enabled());
        window.set_orphaned_receipt(self.acknowledge_enabled());
        window.set_status(self.message.as_str().into());
        // The header speaks only when something is wrong; a working setup shows nothing.
        window.set_holuto_text(
            if self.connected {
                ""
            } else {
                self.holuto_text()
            }
            .into(),
        );
        let printer_problem = self.printer_problem();
        window.set_printer_text(printer_problem.as_deref().unwrap_or_default().into());
        window.set_printer_tone(
            if self.blocked || self.device.as_ref().is_some_and(|status| !status.ready()) {
                2
            } else {
                0
            },
        );
        window.set_processing_state(self.processing_text().into());
        window.set_holuto_state(self.holuto_text().into());
        window.set_printer_identity(self.printer_identity().into());
        window.set_printer_state(printer_problem.unwrap_or_else(|| "Gotowa".into()).into());
        window.set_start_hint(self.start_hint().into());
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
        window.set_from_date(self.from.as_str().into());
        window.set_to_date(self.to.as_str().into());
        window.set_summary(self.summary);
        window.set_confirming(self.pending.is_some());
        if let Some(pending) = &self.pending {
            window.set_confirm_title(pending.title.as_str().into());
            window.set_confirm_text(pending.text.as_str().into());
            window.set_confirm_action(pending.action.as_str().into());
        }
    }

    const fn accepting(&self) -> bool {
        self.connected && !self.stopping && !self.in_flight
    }

    /// Fiscal reports need an accepting worker and no unresolved fiscal block.
    const fn reports_unblocked(&self) -> bool {
        self.accepting() && !self.blocked
    }

    fn reports_enabled(&self) -> bool {
        self.reports_unblocked()
            && self
                .device
                .as_ref()
                .is_some_and(Device::periodic_reports_allowed)
    }

    fn daily_report_enabled(&self) -> bool {
        self.reports_unblocked()
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

    /// Stan's "Obsługa drukarki"; a pairing worker is not processing, so it never stops one.
    pub(crate) const fn processing_text(&self) -> &'static str {
        if self.running && self.stopping {
            "Zatrzymywanie…"
        } else if self.running {
            "Uruchomiona"
        } else if self.starting {
            "Uruchamianie…"
        } else {
            "Zatrzymana"
        }
    }

    /// Stan's "Holuto"; the header repeats it only while not connected.
    pub(crate) const fn holuto_text(&self) -> &'static str {
        if self.connected {
            "Połączono"
        } else if self.running {
            "Łączenie…"
        } else {
            "Nie połączono"
        }
    }

    /// The last connected printer, kept after Stop like the published status.
    pub(crate) fn printer_identity(&self) -> String {
        match self.vendor {
            Some(vendor) if !self.unique_number.is_empty() => {
                let mut identity = format!("{} · {}", vendor_label(vendor), self.unique_number);
                if !self.settings.port.is_empty() {
                    identity.push_str(" · ");
                    identity.push_str(&self.settings.port);
                }
                identity
            },
            _ => "—".into(),
        }
    }

    /// Why the printer cannot print now; `None` once it is connected, ready and unblocked.
    pub(crate) fn printer_problem(&self) -> Option<String> {
        if self.blocked {
            Some("Zablokowana: wymaga rozstrzygnięcia".into())
        } else if let Some(status) = self.device.as_ref().filter(|status| !status.ready()) {
            Some(format!("Niegotowa: {}", status.description()))
        } else if self.connected && self.device.is_some() {
            None
        } else {
            Some("Niegotowa: brak połączenia".into())
        }
    }

    pub(crate) const fn start_hint(&self) -> &'static str {
        if self.running || self.starting {
            ""
        } else {
            "Uruchom obsługę drukarki w Ustawieniach."
        }
    }

    pub(crate) fn request(&mut self, action: Action) {
        // Keys and assistive actions still reach controls behind the open dialog.
        if self.pending.is_some() {
            return;
        }
        self.pending = match action {
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
            Action::Save | Action::Pair | Action::Start | Action::Stop | Action::Quit => None,
        };
    }

    fn periodic_confirmation(&mut self) -> Option<Confirmation> {
        let (from, to) = match report_range(&self.from, &self.to, self.today) {
            Ok(range) => range,
            Err(error) => {
                self.problem(error);
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
        let pending = self.pending.take().filter(|_| accepted)?;
        // Readiness may have changed while the confirmation was open.
        let allowed = match &pending.command {
            Command::DailyReport => self.daily_report_enabled(),
            Command::MonthlyReport { .. } | Command::PeriodicReport { .. } => {
                self.reports_enabled()
            },
            Command::ResolveReport => self.resolve_enabled(),
            Command::AcknowledgeOrphanedReceipt => self.acknowledge_enabled(),
        };
        if !allowed {
            // A closed dialog must not read as a printed report or a resolution.
            self.problem("Nie wysłano: stan drukarki zmienił się przed potwierdzeniem.".into());
            return None;
        }
        self.in_flight = true;
        Some(pending.command)
    }
}

/// The brand as printed on the device and in its documentation.
const fn vendor_label(vendor: Vendor) -> &'static str {
    match vendor {
        Vendor::Posnet => "Posnet",
        Vendor::Novitus => "Novitus",
        Vendor::Elzab => "Elzab",
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
