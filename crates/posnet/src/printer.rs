//! Device state, preflight checks and journaled fiscal operations.

use std::{io, sync::Arc, time::Duration};

use jiff::{civil::Date, fmt::strtime::BrokenDownTime};
use serde::{Deserialize, Serialize};

use crate::{
    Error, decimal,
    frame::Reply,
    receipt::{Receipt, Step, VAT_SLOTS},
    session::{Connection, DEFAULT_TIMEOUT, Link, Session},
    text,
};

const TEMO: &str = "POSNET TEMO ONLINE";
/// A Temo answers `getrealid` well within this; a silent port costs no more during a scan.
const SCAN_TIMEOUT: Duration = Duration::from_millis(500);
const REPORT_TIMEOUT: Duration = Duration::from_secs(120);
const PERIODIC_REPORT_TIMEOUT: Duration = Duration::from_secs(300);
/// The rate the printer reserves for "zwolniona" (exempt): 100 %.
const EXEMPT_BASIS_POINTS: u32 = 10_000;
const MAX_ACTIVE_BASIS_POINTS: u32 = 9_999;

/// Durable evidence of an operation in progress, kept by the caller.
pub trait Journal {
    /// Persists that `command` is about to be sent. Called before every command of a fiscal
    /// operation; an error stops the operation before that command reaches the printer.
    fn dispatching(&mut self, command: &'static str) -> io::Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct VatRate {
    pub slot: u8,
    /// Hundredths of a percent: `2300` is 23 %.
    pub basis_points: u32,
}

impl VatRate {
    /// Rates above 99.99 % other than the exempt 100 % mark an unused slot.
    pub const fn active(self) -> bool {
        self.exempt() || self.basis_points <= MAX_ACTIVE_BASIS_POINTS
    }

    pub const fn exempt(self) -> bool {
        self.basis_points == EXEMPT_BASIS_POINTS
    }

    /// `A · 23%`, `G · zwolniona`, `D · nieaktywna`: the slot letter and its rate.
    pub fn label(self) -> String {
        let letter = char::from(b'A'.saturating_add(self.slot));
        let rate = if self.exempt() {
            "zwolniona".to_owned()
        } else if self.active() {
            format!("{}%", decimal::format(self.basis_points.into(), 2)).replace('.', ",")
        } else {
            "nieaktywna".to_owned()
        };
        format!("{letter} · {rate}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Status {
    pub name: String,
    pub version: String,
    pub serial: String,
    /// Why the printer refuses work; empty when it is ready.
    pub problems: Vec<String>,
    pub vat: Vec<VatRate>,
}

impl Status {
    pub const fn ready(&self) -> bool {
        self.problems.is_empty()
    }

    pub fn description(&self) -> String {
        if self.ready() {
            "Gotowa do drukowania".to_owned()
        } else {
            self.problems.join(". ")
        }
    }

    fn require_ready(&self) -> Result<(), Error> {
        if self.ready() {
            Ok(())
        } else {
            Err(Error::NotReady(self.description()))
        }
    }

    fn require_device(&self, serial: &str) -> Result<(), Error> {
        if self.serial == serial {
            Ok(())
        } else {
            Err(Error::Invalid(format!(
                "Podłączona drukarka {} nie jest drukarką {serial}, której dotyczy operacja.",
                self.serial
            )))
        }
    }
}

/// A fiscal report the operator prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    /// Closes the current day in fiscal memory; the date is the printer's own.
    Daily,
    Monthly {
        year: i16,
        month: i8,
    },
    Periodic {
        from: Date,
        to: Date,
        summary: bool,
    },
}

impl Report {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Daily => "Raport dobowy",
            Self::Monthly { .. } => "Raport miesięczny",
            Self::Periodic { .. } => "Raport okresowy",
        }
    }
}

/// What a completed receipt left behind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printed {
    /// The printer's receipt counter after printing; `None` when it could not be read.
    pub receipt_number: Option<String>,
}

#[derive(Debug)]
pub struct Printer {
    link: Arc<dyn Link>,
    connection: Connection,
}

impl Printer {
    pub fn new(link: Arc<dyn Link>, connection: Connection) -> Self {
        Self { link, connection }
    }

    pub const fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Identity, status and VAT table; read-only.
    pub fn probe(&self) -> Result<Status, Error> {
        read_status(&mut self.session()?)
    }

    /// Probes the configured port; when it does not answer and `may_switch` allows it, tries every
    /// other candidate port and adopts the first that identifies itself as a Temo Online. Only
    /// `getrealid` reaches a device that does not, so a scan never disturbs another serial device.
    /// The caller forbids switching while an operation is unresolved: the device must not change.
    pub fn detect(&mut self, may_switch: bool) -> Result<Status, Error> {
        let error = match self.probe() {
            Ok(status) => return Ok(status),
            Err(error) => error,
        };
        if !may_switch {
            return Err(error);
        }
        let mut denied = error.is_denied().then(|| self.connection.port.clone());
        for port in self.link.candidates() {
            if port == self.connection.port {
                continue;
            }
            let connection = Connection {
                port,
                baud_rate: self.connection.baud_rate,
            };
            match identify(self.link.as_ref(), &connection) {
                Ok(Some(status)) => {
                    self.connection = connection;
                    return Ok(status);
                },
                Ok(None) => {},
                Err(scan) => {
                    if denied.is_none() && scan.is_denied() {
                        denied = Some(connection.port);
                    }
                },
            }
        }
        Err(denied.map_or(error, |port| {
            Error::Invalid(format!(
                "Brak uprawnień do portu {port}. Na Linuksie dodaj użytkownika do grupy dialout i \
                 zaloguj się ponownie."
            ))
        }))
    }

    /// The paired device, connected and ready: the precondition for an operator to confirm what
    /// happened to an interrupted operation on it.
    pub fn verify(&self, serial: &str) -> Result<Status, Error> {
        let status = self.probe()?;
        status.require_ready()?;
        status.require_device(serial)?;
        Ok(status)
    }

    /// Prints one receipt on the device `expected` describes. In the same session the printer must
    /// still be that device, ready, with the VAT table the caller validated the receipt against.
    pub fn print_receipt(
        &self,
        receipt: &Receipt,
        expected: &Status,
        journal: &mut dyn Journal,
    ) -> Result<Printed, Error> {
        let mut session = self.session()?;
        let status = read_status(&mut session)?;
        status.require_ready()?;
        status.require_device(&expected.serial)?;
        if status.vat != expected.vat {
            return Err(Error::Invalid(
                "Stawka VAT zmieniła się na drukarce. Odśwież połączenie i sprawdź stawki."
                    .to_owned(),
            ));
        }
        let inactive = receipt.slots().any(|slot| {
            status
                .vat
                .get(usize::from(slot.get()))
                .is_none_or(|rate| !rate.active())
        });
        if inactive {
            return Err(Error::Invalid(
                "Wybrana stawka VAT jest nieaktywna na drukarce.".to_owned(),
            ));
        }
        let header = session.command("hdrget", &[], DEFAULT_TIMEOUT)?;
        let header = text::header_text(header.get("tx").unwrap_or_default()).to_lowercase();
        let footer = (!header.contains(&receipt.brand().to_lowercase())).then_some(receipt.brand());
        dispatch(&mut session, &receipt.steps(footer), journal)?;
        // The receipt is printed; a failure to read its number is reported, never retried.
        let receipt_number = session
            .command("scnt", &[], DEFAULT_TIMEOUT)
            .ok()
            .and_then(|counters| counters.get("bt").map(str::to_owned))
            .filter(|number| !number.is_empty());
        Ok(Printed { receipt_number })
    }

    /// Prints a fiscal report on the paired device `serial`.
    pub fn print_report(
        &self,
        report: Report,
        serial: &str,
        journal: &mut dyn Journal,
    ) -> Result<(), Error> {
        if let Report::Periodic { from, to, .. } = report
            && from > to
        {
            return Err(Error::Invalid(
                "Data początkowa nie może być późniejsza niż końcowa.".to_owned(),
            ));
        }
        let mut session = self.session()?;
        let status = read_status(&mut session)?;
        status.require_ready()?;
        status.require_device(serial)?;
        let step = match report {
            Report::Daily => {
                let clock = session.command("rtcget", &[], DEFAULT_TIMEOUT)?;
                // `da` is `YYYY-MM-DD;HH:MM`; the report takes only the date.
                let day = clock
                    .get("da")
                    .and_then(|value| BrokenDownTime::parse_prefix("%Y-%m-%d", value).ok())
                    .and_then(|(time, _)| time.to_date().ok())
                    .ok_or_else(|| {
                        Error::Protocol("Nie można odczytać daty drukarki.".to_owned())
                    })?;
                Step {
                    command: "dailyrep",
                    params: vec![("da", day.to_string())],
                    timeout: REPORT_TIMEOUT,
                }
            },
            Report::Monthly { year, month } => {
                let first = Date::new(year, month, 1).map_err(|error| {
                    Error::Invalid(format!("Nieprawidłowy miesiąc raportu: {error}."))
                })?;
                Step {
                    command: "monthlyrep",
                    params: vec![("da", first.to_string()), ("su", "0".to_owned())],
                    timeout: REPORT_TIMEOUT,
                }
            },
            Report::Periodic { from, to, summary } => Step {
                command: "periodicrepbydates",
                params: vec![
                    ("fd", from.to_string()),
                    ("td", to.to_string()),
                    ("su", if summary { "1" } else { "0" }.to_owned()),
                ],
                timeout: PERIODIC_REPORT_TIMEOUT,
            },
        };
        dispatch(&mut session, &[step], journal)
    }

    fn session(&self) -> Result<Session, Error> {
        Session::open(self.link.as_ref(), &self.connection)
    }
}

/// Journals and sends each step once. After the first journal entry any failure is uncertain:
/// earlier commands may have changed the printer's state.
fn dispatch(session: &mut Session, steps: &[Step], journal: &mut dyn Journal) -> Result<(), Error> {
    for (index, step) in steps.iter().enumerate() {
        if let Err(error) = journal.dispatching(step.command) {
            return Err(if index == 0 {
                Error::Journal(error)
            } else {
                uncertain(Error::Journal(error))
            });
        }
        let _reply = session
            .command(step.command, &step.params, step.timeout)
            .map_err(uncertain)?;
    }
    Ok(())
}

fn uncertain(error: Error) -> Error {
    Error::Uncertain(Box::new(error))
}

/// `Some(status)` when the port answers as a Temo Online.
fn identify(link: &dyn Link, connection: &Connection) -> Result<Option<Status>, Error> {
    let mut session = Session::open(link, connection)?;
    let identity = session.command("getrealid", &[], SCAN_TIMEOUT)?;
    if identity.get("nm") != Some(TEMO) {
        return Ok(None);
    }
    read_status(&mut session).map(Some)
}

fn read_status(session: &mut Session) -> Result<Status, Error> {
    let identity = session.command("getrealid", &[], DEFAULT_TIMEOUT)?;
    let device = session.command("sdev", &[], DEFAULT_TIMEOUT)?;
    let mechanism = session.command("sprn", &[], DEFAULT_TIMEOUT)?;
    let common = session.command("scomm", &[], DEFAULT_TIMEOUT)?;
    let transaction = session.command("strns", &[], DEFAULT_TIMEOUT)?;
    let vat = session.command("vatget", &[], DEFAULT_TIMEOUT)?;
    status(&identity, &device, &mechanism, &common, &transaction, &vat).map_err(|error| {
        Error::Protocol(format!(
            "Niepełna lub nieprawidłowa odpowiedź statusu drukarki: {error}"
        ))
    })
}

fn status(
    identity: &Reply,
    device: &Reply,
    mechanism: &Reply,
    common: &Reply,
    transaction: &Reply,
    vat: &Reply,
) -> Result<Status, Error> {
    let name = identity.require("nm")?.to_owned();
    let version = identity.require("vr")?.to_owned();
    let serial = identity.require("nu")?.to_owned();
    let fiscal = boolean(common.require("fs")?)?;
    let header = boolean(common.require("hr")?)?;
    let queue_empty = boolean(device.require("qe")?)?;
    let transaction_open =
        transaction.require("to")? != "0" || boolean(transaction.require("fe")?)?;
    let rates = (0..VAT_SLOTS)
        .map(|slot| {
            let key = format!("v{}", char::from(b'a'.saturating_add(slot)));
            let raw = vat.require(&key)?;
            let basis_points = decimal::parse(&raw.replace(',', "."), 2)
                .and_then(|units| u32::try_from(units).ok())
                .ok_or_else(|| Error::Protocol(format!("nieprawidłowa stawka VAT {raw:?}")))?;
            Ok(VatRate { slot, basis_points })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let mut problems = Vec::new();
    if name != TEMO || serial.trim().is_empty() {
        problems.push("Wybrane urządzenie nie jest drukarką Temo Online".to_owned());
    }
    if !fiscal {
        problems.push("Drukarka nie jest w trybie fiskalnym".to_owned());
    }
    if !header {
        problems.push("Drukarka nie ma zaprogramowanego nagłówka".to_owned());
    }
    if device.require("ds")? != "0" {
        problems.push("Drukarka oczekuje na operatora lub jest w menu".to_owned());
    }
    if !queue_empty {
        problems.push("Drukarka ma oczekujące polecenia".to_owned());
    }
    match mechanism.require("pr")? {
        "0" => {},
        "1" => problems.push("Podniesiona dźwignia".to_owned()),
        "3" => problems.push("Otwarta pokrywa".to_owned()),
        "5" => problems.push("Brak papieru".to_owned()),
        "6" => problems.push("Nieprawidłowa temperatura lub zasilanie".to_owned()),
        other => problems.push(format!("Błąd mechanizmu: {other}")),
    }
    if transaction_open || common.require("ts")? != "0" {
        problems.push("Otwarta transakcja. Dokończ lub anuluj ją na drukarce".to_owned());
    }
    if !rates.iter().any(|rate| rate.active()) {
        problems.push("Brak aktywnych stawek VAT".to_owned());
    }
    Ok(Status {
        name,
        version,
        serial,
        problems,
        vat: rates,
    })
}

fn boolean(value: &str) -> Result<bool, Error> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "y" | "t" => Ok(true),
        "0" | "n" | "f" => Ok(false),
        _ => Err(Error::Protocol(format!(
            "nieprawidłowy status logiczny {value:?}"
        ))),
    }
}

#[cfg(test)]
mod tests;
