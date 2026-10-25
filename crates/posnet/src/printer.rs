use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use fiscal::{
    decimal,
    device::{Device, VatKind, VatRate, Vendor},
    driver::Driver,
    error::Error,
    journal::{Journal, Printed},
    link::{Connection, DEFAULT_TIMEOUT, Link},
    receipt::{Receipt, VAT_SLOTS},
    report::Report,
};
use jiff::{Timestamp, civil::Date, fmt::strtime::BrokenDownTime, tz::TimeZone};

use crate::{
    capabilities::capabilities,
    frame::Reply,
    receipt::{self, Step},
    session::{self, Session},
    text,
};

const TEMO: &str = "POSNET TEMO ONLINE";
/// Bounds scan delays on silent ports; Temo answers `getrealid` within this.
const SCAN_TIMEOUT: Duration = Duration::from_millis(500);
const REPORT_TIMEOUT: Duration = Duration::from_secs(120);
const PERIODIC_REPORT_TIMEOUT: Duration = Duration::from_secs(300);
/// Maximum operator wait (paper, cover or key) while a command executes.
const OPERATOR_WAIT: Duration = Duration::from_secs(600);
const WATCH_INTERVAL: Duration = Duration::from_secs(2);
pub(crate) const PRINTER_ZONE: &str = "Europe/Warsaw";
/// The printer encodes VAT exemption as 100 %.
const EXEMPT_BASIS_POINTS: u32 = 10_000;
/// Above 99.99 %, only the exempt 100 % slot is active.
const MAX_ACTIVE_BASIS_POINTS: u32 = 9_999;

#[derive(Debug)]
pub struct Printer {
    link: Arc<dyn Link>,
    connection: Connection,
}

impl Printer {
    pub fn new(link: Arc<dyn Link>, connection: Connection) -> Self {
        Self { link, connection }
    }

    /// Probes the configured port, then other candidates if `may_switch` permits.
    /// Adopts the first Temo Online; unidentified devices receive only `getrealid`.
    /// Switching must be forbidden while an operation is unresolved.
    pub fn detect(&mut self, may_switch: bool) -> Result<Device, Error> {
        let error = match self.probe() {
            Ok(device) => return Ok(device),
            Err(error) => error,
        };
        if !may_switch {
            return Err(error);
        }
        let mut denied = error.is_denied().then(|| self.connection.port.clone());
        for candidate in self.link.candidates() {
            if candidate.port == self.connection.port {
                continue;
            }
            let connection = Connection {
                port: candidate.port,
                baud_rate: self.connection.baud_rate,
            };
            match identify(self.link.as_ref(), &connection) {
                Ok(Some(device)) => {
                    self.connection = connection;
                    return Ok(device);
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

    fn session(&self) -> Result<Session, Error> {
        Session::open(self.link.as_ref(), &self.connection)
    }
}

impl Driver for Printer {
    fn vendor(&self) -> Vendor {
        Vendor::Posnet
    }

    fn connection(&self) -> &Connection {
        &self.connection
    }

    fn probe(&self) -> Result<Device, Error> {
        read_device(&mut self.session()?)
    }

    /// Requires the paired device to be ready before resolving an interrupted operation.
    fn verify(&self, serial: &str) -> Result<Device, Error> {
        let device = self.probe()?;
        device.require_ready()?;
        device.require_serial(serial)?;
        Ok(device)
    }

    /// Rechecks identity, readiness and expected VAT table in the printing session.
    #[tracing::instrument(skip_all, fields(sentry.op = "device.receipt"))]
    fn print_receipt(
        &self,
        receipt: &Receipt,
        expected: &Device,
        journal: &mut dyn Journal,
    ) -> Result<Printed, Error> {
        let mut session = self.session()?;
        let device = read_device(&mut session)?;
        device.require_ready()?;
        device.require_serial(&expected.serial)?;
        if device.vat != expected.vat {
            return Err(Error::Invalid(
                "Stawka VAT zmieniła się na drukarce. Odśwież połączenie i sprawdź stawki."
                    .to_owned(),
            ));
        }
        let inactive = receipt.sales().keys().any(|slot| {
            device
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
        dispatch(&mut session, &receipt::steps(receipt, footer), journal)?;
        // Printing succeeded; an unreadable counter must not trigger a retry.
        let receipt_number = session
            .command("scnt", &[], DEFAULT_TIMEOUT)
            .ok()
            .and_then(|counters| counters.get("bt").map(str::to_owned))
            .filter(|number| !number.is_empty());
        tracing::info!("Receipt printed");
        Ok(Printed {
            receipt_number,
            issued_at,
        })
    }

    #[tracing::instrument(skip_all, fields(sentry.op = "device.report", report = report.label()))]
    fn print_report(
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
        let device = read_device(&mut session)?;
        device.require_ready()?;
        device.require_serial(serial)?;
        let step = match report {
            Report::Daily => {
                let clock = session.command("rtcget", &[], DEFAULT_TIMEOUT)?;
                // `da`: `YYYY-MM-DD;HH:MM`; reports take only the date.
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
        dispatch(&mut session, &[step], journal)?;
        tracing::info!("Report printed");
        Ok(())
    }
}

/// After dispatch, failures are uncertain unless no fiscal record can have been issued.
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
            Err(error) if index == 0 && refusal_is_certain(&error, step.command) => {
                return Err(error);
            },
            Err(error) => return Err(uncertain(error)),
        }
    }
    Ok(())
}

fn refusal_is_certain(error: &Error, sent: &str) -> bool {
    // A trinit error cannot have issued a receipt: only trend makes the fiscal record.
    // Any transaction left open is refused by the next preflight; never reissue this command.
    // Reports ARE the fiscal record, so only an ERR frame rejection (spec pp. 13–15) is certain.
    refuses(error, sent)
        && (sent == "trinit"
            || (matches!(sent, "dailyrep" | "monthlyrep" | "periodicrepbydates")
                && matches!(error, Error::Device { rejected: true, .. })))
}

fn refuses(error: &Error, sent: &str) -> bool {
    matches!(error, Error::Device { command, .. } if command == sent)
}

fn uncertain(error: Error) -> Error {
    tracing::error!("Printer operation has an uncertain outcome");
    Error::Uncertain(Box::new(error))
}

/// Sends once. Replies follow execution (spec p. 15); on timeout, query operator-wait status.
///
/// Wait up to [`OPERATOR_WAIT`], notifying `journal`; after intervention, allow the full step time.
/// Silence or no operator-wait state remains a timeout.
fn execute(session: &mut Session, step: &Step, journal: &mut dyn Journal) -> Result<Reply, Error> {
    let command = step.command;
    let mut deadline = session.send(command, &step.params, step.timeout)?;
    let mut since: Option<(Instant, Duration)> = None;
    let mut shown: Option<String> = None;
    let answered = (|| {
        loop {
            let cap = since.map(|(since, limit)| since + limit);
            if let Some(cap) = cap {
                if Instant::now() >= cap {
                    return Err(Error::Timeout(command));
                }
                deadline = deadline.min(cap);
            }
            match session.receive(command, deadline) {
                Err(Error::Timeout(_)) => {
                    tracing::debug!(command, "Printer reply deadline elapsed");
                },
                answered => break answered,
            }
            match watch(session, command, cap)? {
                Watch::Answered(answered) => break answered,
                Watch::Waiting(problem) => {
                    let (since, limit) = since.get_or_insert_with(|| {
                        (Instant::now(), session.operator_wait_limit(OPERATOR_WAIT))
                    });
                    if since.elapsed() >= *limit {
                        return Err(Error::Timeout(command));
                    }
                    if shown.as_ref() != Some(&problem) {
                        tracing::warn!(command, "Printer needs operator intervention");
                        journal.waiting(Some(&problem));
                        shown = Some(problem);
                    }
                    deadline = Instant::now() + session.timeout(WATCH_INTERVAL);
                },
                Watch::Ready if shown.is_some() => {
                    shown = None;
                    journal.waiting(None);
                    tracing::info!(command, "Printer resumed after operator intervention");
                    deadline = Instant::now() + session.timeout(step.timeout);
                },
                Watch::Ready | Watch::Unanswered => return Err(Error::Timeout(command)),
            }
        }
    })();
    if shown.is_some() {
        journal.waiting(None);
    }
    if let Err(Error::Device { code, .. }) = &answered {
        tracing::warn!(command, device_error_code = code, "Printer refused command");
    }
    answered
}

enum Watch {
    /// The command's own reply arrived meanwhile.
    Answered(Result<Reply, Error>),
    Waiting(String),
    Ready,
    /// At least one status query went unanswered.
    Unanswered,
}

/// `!sprn` and `!sdev` answer during execution (spec p. 15).
/// Echoed tokens distinguish their replies (p. 11).
fn watch(
    session: &mut Session,
    command: &'static str,
    cap: Option<Instant>,
) -> Result<Watch, Error> {
    let mechanism_token = query(session, "!sprn")?;
    let device_token = query(session, "!sdev")?;
    let deadline = Instant::now() + session.timeout(DEFAULT_TIMEOUT);
    let deadline = cap.map_or(deadline, |cap| deadline.min(cap));
    let (mut mechanism, mut device, mut answered) = (None, None, None);
    let mut outstanding = 2_u8;
    while outstanding > 0 {
        let (name, reply) = match session.next_frame(command, deadline) {
            Ok(frame) => frame,
            Err(Error::Timeout(_)) => break,
            Err(error) => return Err(error),
        };
        let token = reply.get("@");
        if token == Some(mechanism_token.as_str()) {
            mechanism = reply.into_result(&name).ok();
            outstanding = outstanding.saturating_sub(1);
        } else if token == Some(device_token.as_str()) {
            device = reply.into_result(&name).ok();
            outstanding = outstanding.saturating_sub(1);
        } else if token.is_none() {
            let result = reply.into_result(&name);
            if name == command || result.as_ref().is_err_and(|error| refuses(error, command)) {
                answered = Some(result);
            } else {
                let _reply = result?;
                return Err(Error::Protocol(format!(
                    "Oczekiwano odpowiedzi {command}, otrzymano {name}."
                )));
            }
        }
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

fn query(session: &mut Session, query: &'static str) -> Result<String, Error> {
    let token = session::token();
    let _deadline = session.send(query, &[("@", token.clone())], DEFAULT_TIMEOUT)?;
    Ok(token)
}

/// Operator-fixable mechanism or keypad state (spec pp. 384-385); `None` when ready.
fn operator_problem(mechanism: &Reply, device: &Reply) -> Option<String> {
    let code = mechanism.get("pr").unwrap_or_default();
    if matches!(code, "1" | "3" | "5" | "6") {
        return mechanism_problem(code);
    }
    matches!(device.get("ds"), Some("2" | "3")).then(|| "Drukarka oczekuje na operatora".to_owned())
}

fn identify(link: &dyn Link, connection: &Connection) -> Result<Option<Device>, Error> {
    let mut session = Session::open(link, connection)?;
    let identity = session.command("getrealid", &[], SCAN_TIMEOUT)?;
    if identity.get("nm") != Some(TEMO) {
        return Ok(None);
    }
    read_device(&mut session).map(Some)
}

/// Prefer ISO8601 `tm` (spec p. 17), preserving the autumn-hour offset; seconds round down.
/// Older replies with only civil `da` choose the earlier repeated instant.
fn clock(session: &mut Session) -> Result<Timestamp, Error> {
    let unreadable = |problem: String| {
        Error::Protocol(format!("Nie można odczytać zegara drukarki: {problem}."))
    };
    let reply = session.command("rtcget", &[], DEFAULT_TIMEOUT)?;
    if let Some(value) = reply.get("tm") {
        let timestamp = value
            .parse::<Timestamp>()
            .map_err(|error| unreadable(error.to_string()))?;
        return Timestamp::from_second(timestamp.as_second().div_euclid(60) * 60)
            .map_err(|error| unreadable(error.to_string()));
    }
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

fn read_device(session: &mut Session) -> Result<Device, Error> {
    let identity = session.command("getrealid", &[], DEFAULT_TIMEOUT)?;
    let device = session.command("sdev", &[], DEFAULT_TIMEOUT)?;
    let mechanism = session.command("sprn", &[], DEFAULT_TIMEOUT)?;
    let common = session.command("scomm", &[], DEFAULT_TIMEOUT)?;
    let transaction = session.command("strns", &[], DEFAULT_TIMEOUT)?;
    let vat = session.command("vatget", &[], DEFAULT_TIMEOUT)?;
    device_from(&identity, &device, &mechanism, &common, &transaction, &vat).map_err(|error| {
        Error::Protocol(format!(
            "Niepełna lub nieprawidłowa odpowiedź statusu drukarki: {error}"
        ))
    })
}

fn device_from(
    identity: &Reply,
    device: &Reply,
    mechanism: &Reply,
    common: &Reply,
    transaction: &Reply,
    vat: &Reply,
) -> Result<Device, Error> {
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
            let kind = match basis_points {
                EXEMPT_BASIS_POINTS => VatKind::Exempt,
                ..=MAX_ACTIVE_BASIS_POINTS => VatKind::Rate { basis_points },
                _ => VatKind::Inactive,
            };
            Ok(VatRate { slot, kind })
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
    if boolean(common.require("td")?)? {
        problems.push("Drukarka jest w trybie tylko do odczytu".to_owned());
    }
    if device.require("tl")? != "0" {
        problems.push("Zasoby transakcji są zajęte przez inny interfejs".to_owned());
    }
    if transaction_open || common.require("ts")? != "0" {
        problems.push("Otwarta transakcja. Dokończ lub anuluj ją na drukarce".to_owned());
    }
    if !rates.iter().any(|rate| rate.active()) {
        problems.push("Brak aktywnych stawek VAT".to_owned());
    }
    Ok(Device {
        vendor: Vendor::Posnet,
        model: name,
        firmware: version,
        serial,
        problems,
        vat: rates,
        capabilities: capabilities(),
    })
}

/// Mechanism `pr` codes (spec p. 384); `None` when ready.
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
