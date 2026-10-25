//! Native fiscal operations: same-session preflight, durable intent before every
//! send, no retries. DLE2 byte order is intentionally never inferred.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use fiscal::{
    decimal,
    device::{Capabilities, Device, Problem, VatKind, VatRate, Vendor},
    driver::Driver,
    error::Error,
    journal::{Journal, Printed},
    link::{Connection, Link},
    receipt::Receipt,
    report::Report,
};
use jiff::{
    Timestamp,
    civil::{Date, DateTime},
    tz::TimeZone,
};

use crate::{
    frame,
    profile::{self, Profile},
    receipt::{self, Step},
    session::{DLE, DleStatus, ENQ, EnqStatus, Session},
};

const TIMEOUT: Duration = Duration::from_secs(3);
const OPERATOR_WAIT: Duration = Duration::from_secs(600);

#[derive(Debug)]
pub struct Printer {
    link: Arc<dyn Link>,
    connection: Connection,
}
impl Printer {
    pub fn new(link: Arc<dyn Link>, connection: Connection) -> Self {
        Self { link, connection }
    }

    fn session(&self) -> Result<Session, Error> {
        Session::open(self.link.as_ref(), &self.connection)
    }
}
impl Driver for Printer {
    fn vendor(&self) -> Vendor {
        Vendor::Novitus
    }

    fn connection(&self) -> &Connection {
        &self.connection
    }

    fn probe(&self) -> Result<Device, Error> {
        Ok(preflight(&mut self.session()?)?.device)
    }

    fn verify(&self, serial: &str) -> Result<Device, Error> {
        let device = self.probe()?;
        device.require_ready()?;
        device.require_serial(serial)?;
        Ok(device)
    }

    fn print_receipt(
        &self,
        receipt: &Receipt,
        expected: &Device,
        journal: &mut dyn Journal,
    ) -> Result<Printed, Error> {
        let mut session = self.session()?;
        let checked = preflight(&mut session)?;
        checked.device.require_ready()?;
        checked.device.require_serial(&expected.serial)?;
        if expected.vendor != Vendor::Novitus || checked.device.vat != expected.vat {
            return Err(Error::Invalid(
                "Drukarka lub stawki VAT zmieniły się. Odśwież połączenie.".to_owned(),
            ));
        }
        if receipt.sales().keys().any(|slot| {
            checked
                .device
                .vat
                .get(usize::from(slot.get()))
                .is_none_or(|rate| !rate.active())
        }) {
            return Err(Error::Invalid(
                "Wybrana stawka VAT jest nieaktywna.".to_owned(),
            ));
        }
        let profile = checked
            .profile
            .ok_or_else(|| Error::NotReady("Nieznany profil drukarki Novitus.".to_owned()))?;
        let footer = (!checked
            .header
            .to_lowercase()
            .contains(&receipt.brand().to_lowercase()))
        .then_some(receipt.brand());
        let steps = receipt::steps(receipt, footer, &profile)?;
        dispatch(&mut session, &steps, journal)?;
        // Readback is evidence only: it never turns successful printing into a retry.
        let receipt_number = session.command("0;0?l", TIMEOUT).ok().and_then(|reply| {
            let mut fields = reply.body.strip_prefix("0?l")?.split('/');
            if fields.next()? != "2" {
                return None;
            }
            let _kind = fields.next()?;
            let number = fields.next()?;
            (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| number.to_owned())
        });
        Ok(Printed {
            receipt_number,
            issued_at: checked.clock,
        })
    }

    fn print_report(
        &self,
        report: Report,
        serial: &str,
        journal: &mut dyn Journal,
    ) -> Result<(), Error> {
        let mut session = self.session()?;
        let checked = preflight(&mut session)?;
        checked.device.require_ready()?;
        checked.device.require_serial(serial)?;
        let today = checked.date;
        let (command, body, timeout) = match report {
            Report::Daily => (
                "#r",
                format!("1;{}#r", date_fields(today)?),
                Duration::from_secs(120),
            ),
            Report::Monthly { year, month } => {
                let from =
                    Date::new(year, month, 1).map_err(|error| Error::Invalid(error.to_string()))?;
                let to = from.last_of_month();
                if to >= today {
                    return Err(Error::Invalid(
                        "Raport miesięczny wymaga zakończonego miesiąca.".to_owned(),
                    ));
                }
                (
                    "#o",
                    format!("{};{};6#o", date_fields(from)?, date_fields(to)?),
                    Duration::from_secs(300),
                )
            },
            Report::Periodic { from, to, summary } => {
                if from > to || to > today {
                    return Err(Error::Invalid(
                        "Nieprawidłowy zakres dat raportu.".to_owned(),
                    ));
                }
                (
                    "#o",
                    format!(
                        "{};{};{}#o",
                        date_fields(from)?,
                        date_fields(to)?,
                        u8::from(summary)
                    ),
                    Duration::from_secs(300),
                )
            },
        };
        dispatch(
            &mut session,
            &[Step {
                command,
                body,
                timeout,
            }],
            journal,
        )
    }
}

/// Discovery sends no fiscal traffic. Reject PC2 before PC1-only ^c reads.
pub fn identify(link: &dyn Link, connection: &Connection) -> Result<Option<Device>, Error> {
    let mut session = Session::open_discovery(link, connection)?;
    let (model, firmware) = identity(&mut session)?;
    if !model.starts_with("NOVITUS ") {
        return Ok(None);
    }
    let endpoint = session.command("50$i", TIMEOUT)?;
    if endpoint.body != "50$IPC1\r" {
        return Ok(None);
    }
    preflight_identity(&mut session, model, firmware, true).map(|checked| Some(checked.device))
}

struct Checked {
    device: Device,
    profile: Option<Profile>,
    header: String,
    clock: Timestamp,
    date: Date,
}
fn identity(session: &mut Session) -> Result<(String, String), Error> {
    let reply = session.command("30#v", TIMEOUT)?;
    let (model, firmware) = reply
        .body
        .strip_prefix("1#R")
        .and_then(|value| value.split_once('/'))
        .ok_or_else(frame::corrupt)?;
    Ok((model.to_owned(), firmware.to_owned()))
}
fn preflight(session: &mut Session) -> Result<Checked, Error> {
    let (model, firmware) = identity(session)?;
    let endpoint = session.command("50$i", TIMEOUT)?;
    preflight_identity(session, model, firmware, endpoint.body == "50$IPC1\r")
}
fn preflight_identity(
    session: &mut Session,
    model: String,
    firmware: String,
    pc1: bool,
) -> Result<Checked, Error> {
    let mut profile = profile::lookup(&model, &firmware);
    let capabilities = profile
        .as_ref()
        .map_or_else(unqualified_capabilities, |profile| {
            profile.capabilities.clone()
        });
    let mut device = Device {
        vendor: Vendor::Novitus,
        model,
        firmware,
        serial: String::new(),
        problems: vec![],
        vat: vec![],
        capabilities,
    };
    if profile.is_none() {
        device
            .problems
            .push("Nieznany model lub firmware Novitus".into());
    }
    if !pc1 {
        device
            .problems
            .push("Połączenie nie prowadzi do fiskalnego portu PC1".into());
        return Ok(Checked {
            device,
            profile,
            header: String::new(),
            clock: Timestamp::UNIX_EPOCH,
            date: Date::ZERO,
        });
    }
    let dialect = session.command("0;1^c5/5/", TIMEOUT)?;
    if !matches!(dialect.options.as_slice(), [(5, value)] if value == "1") {
        device
            .problems
            .push("Wymagany jest natywny protokół NOVITUS".into());
    }
    let pages = session.command("0;1^c91/92/", TIMEOUT)?;
    if !matches!(pages.options.as_slice(), [(91, input), (92, output)] if input == "1" && output == "1")
    {
        device
            .problems
            .push("Ustaw wejście i wyjście PC na Windows-1250".into());
    }
    read_readiness(session, &mut device.problems)?;
    let header = session
        .command("^u", TIMEOUT)?
        .body
        .strip_prefix("1#U")
        .ok_or_else(frame::corrupt)?
        .to_owned();
    if header.trim().is_empty() {
        device
            .problems
            .push("Brak zaprogramowanego nagłówka".into());
    }
    let vat = session.command("23#s", TIMEOUT)?;
    let (rates, serial, idle) = vat_table(&vat.body)?;
    device.vat = rates;
    device.serial = serial;
    if !idle {
        device
            .problems
            .push("Status fiskalny wskazuje otwartą transakcję lub błąd".into());
    }
    if !device.vat.iter().any(|rate| rate.active()) {
        device.problems.push("Brak aktywnych stawek VAT".into());
    }
    let (clock, date) = clock(session)?;
    if let Some(profile) = profile.as_mut() {
        read_repertoire(session, profile, &mut device);
    }
    Ok(Checked {
        device,
        profile,
        header,
        clock,
        date,
    })
}

fn read_repertoire(session: &mut Session, profile: &mut Profile, device: &mut Device) {
    match session.command("52$i", TIMEOUT) {
        Ok(reply) => {
            if full_cp1250(&reply.body) {
                profile.enable_full_cp1250();
                device
                    .capabilities
                    .allowed_characters
                    .clone_from(&profile.capabilities.allowed_characters);
            }
        },
        Err(error) => {
            // A matching refusal leaves the session usable with ASCII. Lost or
            // corrupt replies leave it blocked; no fiscal write may follow.
            if !matches!(error, Error::Device { .. }) {
                device
                    .problems
                    .push("Nie można bezpiecznie odczytać możliwości znaków drukarki".into());
            }
        },
    }
}

fn full_cp1250(body: &str) -> bool {
    let Some(fields) = body
        .strip_prefix("52$I")
        .and_then(|body| body.strip_suffix('\r'))
    else {
        return false;
    };
    let fields = fields.split('\r');
    fields.clone().count() == 8
        && fields
            .clone()
            .all(|field| field.len() == 8 && field.bytes().all(|byte| byte.is_ascii_hexdigit()))
        && fields
            .clone()
            .next()
            .and_then(|field| u32::from_str_radix(field, 16).ok())
            .is_some_and(|attr1| attr1 & 0x0200 != 0)
}
const fn unqualified_capabilities() -> Capabilities {
    Capabilities {
        max_item_name: 0,
        max_lines: 0,
        max_company_name: 0,
        max_buyer_nip: 0,
        max_quantity_decimals: 0,
        allowed_characters: String::new(),
        advance_deductions: false,
        zero_total_receipts: false,
        company_footer: false,
    }
}
fn read_readiness(session: &mut Session, problems: &mut Vec<Problem>) -> Result<(), Error> {
    let mode = session.command("11$i", TIMEOUT)?;
    let fields = mode
        .body
        .strip_prefix("11$I")
        .ok_or_else(frame::corrupt)?
        .split('\r');
    if fields.clone().count() != 9 {
        return Err(frame::corrupt());
    }
    if fields.clone().next() != Some("1")
        && !(cfg!(workshop_acceptance) && fields.clone().next() == Some("0"))
    {
        problems.push("Drukarka nie jest w czynnym trybie fiskalnym".into());
    }
    if fields.clone().nth(5) != Some("0") {
        problems.push("Nieobsługiwany typ drukarki fiskalnej".into());
    }
    let dle = DleStatus::decode(session.realtime(DLE)?)?;
    let enq = EnqStatus::decode(session.realtime(ENQ)?)?;
    let extended = session.extended_status()?;
    // Zero flags prove idle without guessing DLE2's byte-label ordering.
    if !dle.online
        || dle.paper_or_battery
        || dle.mechanism_error
        || (!enq.fiscal && !cfg!(workshop_acceptance))
        || enq.transaction
        || extended.flags_wire != [0; 4]
        || extended.last_error != 0
    {
        problems.push("Drukarka jest zajęta, ma otwarty dokument lub wymaga obsługi".into());
    }
    Ok(())
}
fn vat_table(body: &str) -> Result<(Vec<VatRate>, String, bool), Error> {
    let mut fields = body
        .strip_prefix("2#X")
        .ok_or_else(frame::corrupt)?
        .split('/');
    let status: Vec<_> = fields
        .next()
        .ok_or_else(frame::corrupt)?
        .split(';')
        .collect();
    if status.len() != 9
        || status
            .iter()
            .any(|value| value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(frame::corrupt());
    }
    let mut rates = Vec::with_capacity(7);
    for slot in 0..7 {
        let value = decimal::parse(fields.next().ok_or_else(frame::corrupt)?, 2)
            .ok_or_else(frame::corrupt)?;
        let kind = match value {
            9899 | 10000 => VatKind::Exempt,
            9999 | 10100 => VatKind::Inactive,
            0..=9998 => VatKind::Rate {
                basis_points: u32::try_from(value).map_err(|_error| frame::corrupt())?,
            },
            _ => return Err(frame::corrupt()),
        };
        rates.push(VatRate { slot, kind });
    }
    // Receipt count, seven totalizers, returnable packages, then unique number.
    for _ in 0..9 {
        let _value = decimal::parse(fields.next().ok_or_else(frame::corrupt)?, 2)
            .ok_or_else(frame::corrupt)?;
    }
    let serial = fields.next().ok_or_else(frame::corrupt)?;
    if serial.len() != 13
        || !serial.bytes().all(|byte| byte.is_ascii_alphanumeric())
        || fields.next().is_some()
    {
        return Err(frame::corrupt());
    }
    Ok((
        rates,
        serial.to_owned(),
        status.first() == Some(&"0")
            && (status.get(1) == Some(&"1")
                || (cfg!(workshop_acceptance) && status.get(1) == Some(&"0")))
            && status.get(2) == Some(&"0"),
    ))
}
fn clock(session: &mut Session) -> Result<(Timestamp, Date), Error> {
    let body = session.command("#c", TIMEOUT)?.body;
    let fields = body
        .strip_prefix("1#C")
        .ok_or_else(frame::corrupt)?
        .split(';')
        .map(|value| {
            if value.is_empty()
                || value.len() > 2
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(frame::corrupt());
            }
            value.parse::<i8>().map_err(|_error| frame::corrupt())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let [year, month, day, hour, minute, second] = fields.as_slice() else {
        return Err(frame::corrupt());
    };
    if !(0..=49).contains(year) {
        return Err(frame::corrupt());
    }
    let datetime = DateTime::new(
        2000 + i16::from(*year),
        *month,
        *day,
        *hour,
        *minute,
        *second,
        0,
    )
    .map_err(|_error| frame::corrupt())?;
    // No offset exists on the wire: fail on DST gaps/folds instead of guessing.
    let timestamp = TimeZone::get("Europe/Warsaw")
        .and_then(|zone| zone.to_ambiguous_timestamp(datetime).unambiguous())
        .map_err(|error| Error::Protocol(format!("Nieprawidłowy zegar drukarki: {error}.")))?;
    let rounded = Timestamp::from_second(timestamp.as_second().div_euclid(60) * 60)
        .map_err(|_error| frame::corrupt())?;
    Ok((rounded, datetime.date()))
}
fn date_fields(date: Date) -> Result<String, Error> {
    if !(2000..=2099).contains(&date.year()) {
        return Err(Error::Invalid(
            "Rok raportu musi należeć do zakresu 2000–2099.".to_owned(),
        ));
    }
    Ok(format!(
        "{};{};{}",
        date.year() - 2000,
        date.month(),
        date.day()
    ))
}
fn dispatch(session: &mut Session, steps: &[Step], journal: &mut dyn Journal) -> Result<(), Error> {
    for (index, step) in steps.iter().enumerate() {
        if let Err(error) = journal.dispatching(step.command) {
            let error = Error::Journal(error);
            return Err(if index == 0 { error } else { uncertain(error) });
        }
        if let Err(error) = execute(session, step, journal) {
            // Only begin prints nothing; only daily 37 documents pre-print cancel.
            let certain = index == 0 && refusal_is_certain(&error, step.command);
            return Err(if certain { error } else { uncertain(error) });
        }
    }
    Ok(())
}
fn refusal_is_certain(error: &Error, sent: &str) -> bool {
    matches!(error, Error::Device { command, code, .. }
        if command == sent && (sent == "$h" || (sent == "#r" && *code == 37)))
}
fn uncertain(error: Error) -> Error {
    Error::Uncertain(Box::new(error))
}

/// DLE2 is read for integrity only; positive wait reasons use documented DLE bits.
/// Never send ENQ (synchronous) or a new frame while the command is pending.
fn execute(session: &mut Session, step: &Step, journal: &mut dyn Journal) -> Result<(), Error> {
    let mut result = session.command(&step.body, step.timeout).map(|_reply| ());
    let mut since = None;
    let mut shown = false;
    while matches!(result, Err(Error::Timeout(_))) {
        let cap = since
            .get_or_insert_with(|| Instant::now() + session.operator_wait_limit(OPERATOR_WAIT));
        if Instant::now() >= *cap {
            break;
        }
        let watched = (|| {
            let _integrity = session.extended_status_until(*cap)?;
            let dle = DleStatus::decode(session.realtime_until(DLE, *cap)?)?;
            if dle.paper_or_battery {
                Ok(Some("Brak papieru lub niskie napięcie akumulatora"))
            } else if dle.mechanism_error {
                Ok(Some("Błąd mechanizmu drukarki"))
            } else {
                Ok(None)
            }
        })();
        match watched {
            Err(error) => {
                result = Err(error);
                break;
            },
            Ok(Some(problem)) => {
                journal.waiting(Some(problem));
                shown = true;
            },
            Ok(None) if shown => {
                journal.waiting(None);
                shown = false;
            },
            // An ACK may have arrived during status reads; only a matching ACK
            // can complete, including when no positive wait reason was found.
            Ok(None) => {},
        }
        let timeout = if shown {
            Duration::from_secs(2)
        } else {
            step.timeout
        };
        result = session.finish_fiscal(
            step.command,
            (Instant::now() + session.timeout(timeout)).min(*cap),
        );
        if !shown && matches!(result, Err(Error::Timeout(_))) {
            break;
        }
    }
    if shown {
        journal.waiting(None);
    }
    result
}

#[cfg(test)]
mod tests;
