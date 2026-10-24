//! Device state, preflight checks and journaled fiscal operations.

use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use jiff::{Timestamp, civil::Date, fmt::strtime::BrokenDownTime, tz::TimeZone};
use serde::{Deserialize, Serialize};

use crate::{
    Error, decimal,
    frame::Reply,
    receipt::{Receipt, Step, VAT_SLOTS},
    session::{self, Connection, DEFAULT_TIMEOUT, Link, Session},
    text,
};

const TEMO: &str = "POSNET TEMO ONLINE";
/// A Temo answers `getrealid` well within this; a silent port costs no more during a scan.
const SCAN_TIMEOUT: Duration = Duration::from_millis(500);
const REPORT_TIMEOUT: Duration = Duration::from_secs(120);
const PERIODIC_REPORT_TIMEOUT: Duration = Duration::from_secs(300);
/// How long a printer executing a command may wait for an operator (paper, cover, a key) before
/// the command counts as timed out.
const OPERATOR_WAIT: Duration = Duration::from_secs(600);
/// How often a printer waiting for an operator is asked again.
const WATCH_INTERVAL: Duration = Duration::from_secs(2);
/// The rate the printer reserves for "zwolniona" (exempt): 100 %.
const EXEMPT_BASIS_POINTS: u32 = 10_000;
const MAX_ACTIVE_BASIS_POINTS: u32 = 9_999;
/// The zone of the printer's clock: Polish civil time.
pub(crate) const PRINTER_ZONE: &str = "Europe/Warsaw";

/// Durable evidence of an operation in progress, kept by the caller, who also hears what the
/// printer waits for while it runs.
pub trait Journal {
    /// Persists that `command` is about to be sent. Called before every command of a fiscal
    /// operation; an error stops the operation before that command reaches the printer.
    fn dispatching(&mut self, command: &'static str) -> io::Result<()>;

    /// The printer, executing the last dispatched command, waits for an operator to fix `problem`
    /// (paper, cover, a key), and the operation waits with it. `None`: fixed, it prints again.
    fn waiting(&mut self, problem: Option<&str>);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// The printer's clock just before the receipt opened, to the minute: never later than the
    /// time it prints.
    pub issued_at: Timestamp,
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
        let issued_at = clock(&mut session)?;
        dispatch(&mut session, &receipt.steps(footer), journal)?;
        // The receipt is printed; a failure to read its number is reported, never retried.
        let receipt_number = session
            .command("scnt", &[], DEFAULT_TIMEOUT)
            .ok()
            .and_then(|counters| counters.get("bt").map(str::to_owned))
            .filter(|number| !number.is_empty());
        Ok(Printed {
            receipt_number,
            issued_at,
        })
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

/// Journals and sends each step once.
///
/// After the first journal entry any failure is uncertain: earlier commands may have changed the
/// printer's state. The one exception is the printer refusing `trinit` itself: no transaction
/// opened, and no receipt exists without one.
fn dispatch(session: &mut Session, steps: &[Step], journal: &mut dyn Journal) -> Result<(), Error> {
    for (index, step) in steps.iter().enumerate() {
        if let Err(error) = journal.dispatching(step.command) {
            return Err(if index == 0 {
                Error::Journal(error)
            } else {
                uncertain(Error::Journal(error))
            });
        }
        match execute(session, step, journal) {
            Ok(_reply) => {},
            Err(error) if index == 0 && refuses_trinit(&error, step.command) => return Err(error),
            Err(error) => return Err(uncertain(error)),
        }
    }
    Ok(())
}

/// Whether `error` is the printer refusing `trinit` itself. It must name `trinit`: a late error
/// reply to another command proves nothing about this one.
fn refuses_trinit(error: &Error, sent: &str) -> bool {
    sent == "trinit" && refuses(error, sent)
}

/// Whether `error` is the printer's error reply to `sent`.
fn refuses(error: &Error, sent: &str) -> bool {
    matches!(error, Error::Device { command, .. } if command == sent)
}

fn uncertain(error: Error) -> Error {
    Error::Uncertain(Box::new(error))
}

/// Sends `step` once and waits for its reply; nothing is ever sent twice.
///
/// The printer replies only once it executed the command (spec p. 15), so when the step's time
/// passes it is asked why. While it waits for an operator the step waits too, up to
/// [`OPERATOR_WAIT`], and `journal` hears the problem; once it is fixed the step gets its full
/// time again. A printer waiting for nothing, or not answering, timed out as before.
fn execute(session: &mut Session, step: &Step, journal: &mut dyn Journal) -> Result<Reply, Error> {
    let command = step.command;
    let mut deadline = session.send(command, &step.params, step.timeout)?;
    let mut since: Option<Instant> = None;
    let mut shown: Option<String> = None;
    let answered = loop {
        match session.receive(command, deadline) {
            Err(Error::Timeout(_)) => {},
            answered => break answered,
        }
        match watch(session, command)? {
            Watch::Answered(answered) => break answered,
            Watch::Waiting(problem) => {
                if since.get_or_insert_with(Instant::now).elapsed() >= OPERATOR_WAIT {
                    return Err(Error::Timeout(command));
                }
                if shown.as_ref() != Some(&problem) {
                    journal.waiting(Some(&problem));
                    shown = Some(problem);
                }
                deadline = Instant::now() + WATCH_INTERVAL;
            },
            // Fixed: the printer is executing the command again, which may take its full time.
            Watch::Ready if since.take().is_some() => {
                shown = None;
                journal.waiting(None);
                deadline = Instant::now() + step.timeout;
            },
            Watch::Ready | Watch::Unanswered => return Err(Error::Timeout(command)),
        }
    };
    if shown.is_some() {
        journal.waiting(None);
    }
    answered
}

/// What a printer still executing a command says about itself.
enum Watch {
    /// The command's own reply arrived meanwhile.
    Answered(Result<Reply, Error>),
    /// It waits for an operator to fix this.
    Waiting(String),
    /// It waits for nothing.
    Ready,
    /// It did not answer both status queries.
    Unanswered,
}

/// Asks a printer executing `command` for its state with `!sprn` and `!sdev`, which it answers at
/// once (spec p. 15). Each carries a token its reply echoes (p. 11), so no reply is mistaken for
/// another.
fn watch(session: &mut Session, command: &'static str) -> Result<Watch, Error> {
    let mechanism_token = query(session, "!sprn")?;
    let device_token = query(session, "!sdev")?;
    let deadline = Instant::now() + DEFAULT_TIMEOUT;
    let (mut mechanism, mut device, mut answered) = (None, None, None);
    let mut outstanding = 2_u8;
    while outstanding > 0 {
        let (name, reply) = match session.next_frame(command, deadline) {
            Ok(frame) => frame,
            Err(Error::Timeout(_)) => break,
            Err(error @ Error::Device { .. }) if refuses(&error, command) => {
                answered = Some(Err(error));
                continue;
            },
            // A refused query: one state fewer to judge by.
            Err(Error::Device { .. }) => {
                outstanding = outstanding.saturating_sub(1);
                continue;
            },
            Err(error) => return Err(error),
        };
        let token = reply.get("@");
        if token == Some(mechanism_token.as_str()) {
            mechanism = Some(reply);
            outstanding = outstanding.saturating_sub(1);
        } else if token == Some(device_token.as_str()) {
            device = Some(reply);
            outstanding = outstanding.saturating_sub(1);
        } else if name == command {
            answered = Some(Ok(reply));
        } else if token.is_none() {
            return Err(Error::Protocol(format!(
                "Oczekiwano odpowiedzi {command}, otrzymano {name}."
            )));
        }
        // Any other token: a late answer to an earlier query.
    }
    if let Some(answered) = answered {
        return Ok(Watch::Answered(answered));
    }
    Ok(match (mechanism, device) {
        (Some(mechanism), Some(device)) => {
            operator_problem(&mechanism, &device).map_or(Watch::Ready, Watch::Waiting)
        },
        _ => Watch::Unanswered,
    })
}

/// Sends an asynchronous status query with a fresh token and returns the token.
fn query(session: &mut Session, query: &'static str) -> Result<String, Error> {
    let token = session::token();
    let _deadline = session.send(query, &[("@", token.clone())], DEFAULT_TIMEOUT)?;
    Ok(token)
}

/// What a printer executing a command waits for an operator to fix: the lever, cover, paper,
/// temperature or power, or a key on its keypad (spec pp. 384-385). `None` when nothing.
fn operator_problem(mechanism: &Reply, device: &Reply) -> Option<String> {
    let code = mechanism.get("pr").unwrap_or_default();
    if matches!(code, "1" | "3" | "5" | "6") {
        return mechanism_problem(code);
    }
    matches!(device.get("ds"), Some("2" | "3")).then(|| "Drukarka oczekuje na operatora".to_owned())
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

/// The printer's clock as an instant.
///
/// `rtcget` answers `da` as `YYYY-MM-DD;HH:MM` in Polish civil time; an hour the autumn change
/// repeats is read as its earlier instant, so the result is never later than the printer's own.
fn clock(session: &mut Session) -> Result<Timestamp, Error> {
    let unreadable = |problem: String| {
        Error::Protocol(format!("Nie można odczytać zegara drukarki: {problem}."))
    };
    let reply = session.command("rtcget", &[], DEFAULT_TIMEOUT)?;
    let value = reply
        .get("da")
        .ok_or_else(|| unreadable("brak parametru da".to_owned()))?;
    let (time, _rest) = BrokenDownTime::parse_prefix("%Y-%m-%d;%H:%M", value)
        .map_err(|error| unreadable(error.to_string()))?;
    let time = time
        .to_datetime()
        .map_err(|error| unreadable(error.to_string()))?;
    TimeZone::get(PRINTER_ZONE)
        .and_then(|zone| zone.to_ambiguous_timestamp(time).earlier())
        .map_err(|error| unreadable(error.to_string()))
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
    problems.extend(mechanism_problem(mechanism.require("pr")?));
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

/// What the mechanism status `pr` (spec p. 384) says is wrong; `None` when nothing is.
fn mechanism_problem(code: &str) -> Option<String> {
    Some(match code {
        "0" => return None,
        "1" => "Podniesiona dźwignia".to_owned(),
        "3" => "Otwarta pokrywa".to_owned(),
        "5" => "Brak papieru".to_owned(),
        "6" => "Nieprawidłowa temperatura lub zasilanie".to_owned(),
        other => format!("Błąd mechanizmu: {other}"),
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
