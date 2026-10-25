//! Same-session identity/readiness/VAT/clock checks before every fiscal operation.
//!
//! Online ySR is never confused with legacy YSR. Only a matched initial LOR
//! transport rejection is certain after dispatch.
use crate::{
    frame,
    profile::{self, Profile},
    receipt::{self, Step},
    session::Session,
};
use fiscal::{
    decimal,
    device::{Device, Problem, VatKind, VatRate, Vendor},
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
use std::{sync::Arc, time::Duration};
const TIMEOUT: Duration = Duration::from_secs(3);
const PRINT_TIMEOUT: Duration = Duration::from_secs(600);

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
        Vendor::Elzab
    }

    fn connection(&self) -> &Connection {
        &self.connection
    }

    fn probe(&self) -> Result<Device, Error> {
        Ok(preflight(&mut self.session()?, false)?.device)
    }

    fn verify(&self, unique_number: &str) -> Result<Device, Error> {
        let device = self.probe()?;
        device.require_resolvable()?;
        device.require_unique_number(unique_number)?;
        Ok(device)
    }

    fn print_receipt(
        &self,
        receipt: &Receipt,
        expected: &Device,
        journal: &mut dyn Journal,
    ) -> Result<Printed, Error> {
        let mut session = self.session()?;
        let checked = preflight(&mut session, false)?;
        checked.device.require_ready()?;
        checked
            .device
            .require_unique_number(&expected.unique_number)?;
        if expected.vendor != Vendor::Elzab || checked.device.vat != expected.vat {
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
            .ok_or_else(|| Error::NotReady("Nieznany profil Elzab.".to_owned()))?;
        let footer = (!checked
            .header
            .to_lowercase()
            .contains(&receipt.brand().to_lowercase()))
        .then_some(receipt.brand());
        let steps = receipt::steps(receipt, footer, &profile)?;
        // LPC's nested error must be checked, even when outer error is empty.
        let permission = query(&mut session, "LPC", "LPC\x1dA1")?;
        let code = field(&permission, 'B')?;
        if !matches!(code, "" | "0") {
            return Err(Error::NotReady(format!(
                "Elzab nie pozwala otworzyć paragonu: {code}."
            )));
        }
        // Read-only evidence; never use a changed counter as proof of our print.
        let _before = read_number(&mut session, "SER", 'J')?;
        let _day = read_number(&mut session, "RNR", 'A')?;
        let issued_at = timestamp(clock(&mut session)?)?;
        dispatch(&mut session, &steps, journal)?;
        let receipt_number = read_number(&mut session, "SER", 'J').ok();
        Ok(Printed {
            receipt_number,
            issued_at,
        })
    }

    fn print_report(
        &self,
        report: Report,
        unique_number: &str,
        journal: &mut dyn Journal,
    ) -> Result<(), Error> {
        let mut session = self.session()?;
        let checked = preflight(&mut session, report == Report::Daily)?;
        checked.device.require_ready()?;
        checked.device.require_unique_number(unique_number)?;
        let (command, body) = match report {
            Report::Daily => {
                let _before = read_number(&mut session, "RNR", 'A')?;
                ("RRP", "RRP\x1dI1\x1dJ1".to_owned())
            },
            Report::Monthly { year, month } => {
                let from = Date::new(year, month, 1).map_err(|e| Error::Invalid(e.to_string()))?;
                if from.last_of_month() >= checked.date {
                    return Err(Error::Invalid(
                        "Raport miesięczny wymaga zakończonego miesiąca.".to_owned(),
                    ));
                }
                let date = date_fields(from)?;
                (
                    "RPP",
                    format!(
                        "RPP\x1dE1\x1dFB{}\x1dS0",
                        date.get(..4).ok_or_else(frame::corrupt)?
                    ),
                )
            },
            Report::Periodic { from, to, summary } => {
                if from > to || to > checked.date {
                    return Err(Error::Invalid(
                        "Nieprawidłowy zakres dat raportu.".to_owned(),
                    ));
                }
                (
                    "RPP",
                    format!(
                        "RPP\x1dE{}\x1dFE{}\x1eF{}\x1dS0",
                        if summary { 3 } else { 1 },
                        date_fields(from)?,
                        date_fields(to)?
                    ),
                )
            },
        };
        dispatch(&mut session, &[Step::new(command, body)], journal)?;
        if command == "RRP" {
            // The protocol promises acceptance, not completion.
            return Err(uncertain(Error::NotReady(
                "Raport dobowy został przyjęty, ale zakończenie wydruku wymaga \
                 sprawdzenia przez operatora."
                    .to_owned(),
            )));
        }
        Ok(())
    }
}

/// XTR model-name matching only; reused legacy numeric IDs never qualify ONLINE.
///
/// An unlisted model named ELZAB is reported by name instead of as an absent printer;
/// other names are not Elzab. Either way no further command is sent.
pub fn identify(link: &dyn Link, connection: &Connection) -> Result<Option<Device>, Error> {
    let mut session = Session::open(link, connection)?;
    let (model, firmware) = identity(&mut session)?;
    if profile::lookup(&model).is_none() {
        if !model
            .get(..5)
            .is_some_and(|brand| brand.eq_ignore_ascii_case("ELZAB"))
        {
            return Ok(None);
        }
        return Err(Error::Unsupported(format!(
            "Nieobsługiwany model drukarki Elzab: {model} (oprogramowanie {firmware}). \
             Obsługiwane są Elzab Zeta, Mera i Cube ONLINE."
        )));
    }
    Ok(Some(
        preflight_identity(&mut session, model, firmware, false)?.device,
    ))
}
struct Checked {
    device: Device,
    profile: Option<Profile>,
    header: String,
    date: Date,
}
fn query(session: &mut Session, command: &'static str, body: &str) -> Result<String, Error> {
    session.command(command, body, TIMEOUT)
}
fn identity(session: &mut Session) -> Result<(String, String), Error> {
    let body = query(session, "XTR", "XTR")?;
    let model = field(&body, 'B')?;
    let firmware = field(&body, 'D')?;
    if model.trim().is_empty() || firmware.trim().is_empty() {
        return Err(frame::corrupt());
    }
    Ok((model.to_owned(), firmware.to_owned()))
}
fn preflight(session: &mut Session, daily: bool) -> Result<Checked, Error> {
    let (model, firmware) = identity(session)?;
    preflight_identity(session, model, firmware, daily)
}
fn preflight_identity(
    session: &mut Session,
    model: String,
    firmware: String,
    daily: bool,
) -> Result<Checked, Error> {
    let mut profile = profile::lookup(&model);
    let capabilities = profile
        .as_ref()
        .map(|p| p.capabilities.clone())
        .unwrap_or_default();
    let mut device = Device {
        vendor: Vendor::Elzab,
        model,
        firmware,
        unique_number: String::new(),
        problems: vec![],
        vat: vec![],
        capabilities,
    };
    if profile.is_none() {
        device.problems.push("Nieznany model Elzab ONLINE".into());
    }
    let unique_number = query(session, "FNR", "FNR")?;
    let unique_number = field(&unique_number, 'B')?;
    if !(1..=14).contains(&unique_number.len())
        || !unique_number.bytes().all(|b| (0x21..=0x7E).contains(&b))
    {
        return Err(frame::corrupt());
    }
    unique_number.clone_into(&mut device.unique_number);
    let page = query(session, "XHR", "XHR\x1dw")?;
    if field(&page, 'w')? != "ACP1250" {
        device
            .problems
            .push("Ustaw stronę kodową drukarki na CP1250".into());
        device.capabilities.allowed_characters.clear();
        if let Some(p) = &mut profile {
            p.capabilities.allowed_characters.clear();
        }
    }
    let status = query(session, "ySR", "ySR")?;
    readiness(&status, &mut device.problems, daily)?;
    let limits = query(session, "YYR", "YYR\x1dJ")?;
    let lines = subfield(field(&limits, 'J')?, 'A')?
        .parse::<usize>()
        .map_err(|_number| frame::corrupt())?;
    if lines == 0 || lines > 256 {
        return Err(frame::corrupt());
    }
    let mut header = String::new();
    for index in 1..=lines {
        let body = query(session, "yHR", &format!("yHR\x1dva{index}"))?;
        let name = subfield(field(&body, 'v')?, 'b')?;
        if !header.is_empty() {
            header.push('\n');
        }
        header.push_str(name);
    }
    let nip = query(session, "yHR", "yHR\x1dx")?;
    let taxpayer = subfield(field(&nip, 'x')?, 'i')?;
    if header.trim().is_empty() || taxpayer.trim().is_empty() {
        device
            .problems
            .push("Brak zaprogramowanego nagłówka lub NIP".into());
    }
    let vat = query(session, "FTR", "FTR")?;
    let table = field(&vat, 'A')?;
    for slot in 0..7 {
        let value = subfield(table, char::from(b'A' + slot))?;
        let kind = match value {
            "=" => VatKind::Exempt,
            "?" => VatKind::Inactive,
            _ => {
                let (whole, fraction) = value.split_once(',').ok_or_else(frame::corrupt)?;
                if whole.is_empty()
                    || fraction.len() != 2
                    || !whole
                        .bytes()
                        .chain(fraction.bytes())
                        .all(|b| b.is_ascii_digit())
                {
                    return Err(frame::corrupt());
                }
                let basis_points = decimal::parse(&value.replace(',', "."), 2)
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n <= 10000)
                    .ok_or_else(frame::corrupt)?;
                VatKind::Rate { basis_points }
            },
        };
        device.vat.push(VatRate { slot, kind });
    }
    if !device.vat.iter().any(|r| r.active()) {
        device.problems.push("Brak aktywnych stawek VAT".into());
    }
    let date = clock(session)?.date();
    Ok(Checked {
        device,
        profile,
        header,
        date,
    })
}
fn readiness(body: &str, problems: &mut Vec<Problem>, daily: bool) -> Result<(), Error> {
    let mut packages = [None; 3];
    for field in body.split('\u{1d}').skip(1) {
        let value = field.strip_prefix('S').ok_or_else(frame::corrupt)?;
        let number: usize = subfield(value, 'A')?
            .parse()
            .map_err(|_number| frame::corrupt())?;
        let value = subfield(value, 'B')?;
        if value.is_empty() || value.len() > 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(frame::corrupt());
        }
        let value = u128::from_str_radix(value, 16).map_err(|_number| frame::corrupt())?;
        let entry = number
            .checked_sub(1)
            .and_then(|i| packages.get_mut(i))
            .ok_or_else(frame::corrupt)?;
        if entry.replace(value).is_some() {
            return Err(frame::corrupt());
        }
    }
    let [Some(one), Some(two), Some(three)] = packages else {
        return Err(frame::corrupt());
    };
    if three > u128::from(u32::MAX) {
        return Err(frame::corrupt());
    }
    let any = |value: u128, bits: &[u32]| bits.iter().any(|bit| value & (1_u128 << bit) != 0);
    if any(one, &[123, 122, 115, 103, 102])
        || any(
            two,
            &[
                122, 101, 100, 99, 90, 87, 82, 81, 78, 77, 70, 68, 67, 66, 65, 64, 0,
            ],
        )
        || (two & 8 != 0 && !cfg!(workshop_acceptance))
        || ![102, 98].iter().all(|bit| two & (1_u128 << bit) != 0)
        || any(three, &[4, 3, 2, 1, 0])
        || three & (1 << 5) == 0
    {
        problems.push(
            "Drukarka Elzab niegotowa: tryb, pamięć, zasilanie, otwarty dokument \
             lub mechanizm/papier"
                .into(),
        );
    }
    // Read-only still permits resolution, which sends no commands. Reports stay refused
    // until a read-only report is qualified.
    if two & 4 == 0 {
        problems.push(Problem::ReadOnly {
            periodic_reports: false,
        });
    }
    // Reminder-only and overdue-daily bits must not prevent the daily report.
    if !daily && two & (1_u128 << 115) != 0 {
        problems.push(Problem::DailyReportOverdue);
    }
    Ok(())
}
/// Civil printer time; only the date is needed outside receipt timestamps.
fn clock(session: &mut Session) -> Result<DateTime, Error> {
    let body = query(session, "XCR", "XCR")?;
    let value = field(&body, 'A')?;
    if value.len() != 12 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(frame::corrupt());
    }
    let pairs: &[[u8; 2]; 6] = value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .try_into()
        .map_err(|_length| frame::corrupt())?;
    let [year, month, day, hour, minute, second] = pairs
        .map(|[a, b]| i8::try_from((a - b'0') * 10 + b - b'0').map_err(|_number| frame::corrupt()));
    let year = year?;
    if !(0..=49).contains(&year) {
        return Err(frame::corrupt());
    }
    DateTime::new(
        2000 + i16::from(year),
        month?,
        day?,
        hour?,
        minute?,
        second?,
        0,
    )
    .map_err(|_date| frame::corrupt())
}
fn timestamp(datetime: DateTime) -> Result<Timestamp, Error> {
    // No offset exists on the wire: fail on DST gaps/folds instead of guessing.
    let timestamp = TimeZone::get("Europe/Warsaw")
        .and_then(|zone| zone.to_ambiguous_timestamp(datetime).unambiguous())
        .map_err(|error| Error::Protocol(format!("Nieprawidłowy zegar drukarki: {error}.")))?;
    Timestamp::from_second(timestamp.as_second().div_euclid(60) * 60)
        .map_err(|_timestamp| frame::corrupt())
}
fn date_fields(date: Date) -> Result<String, Error> {
    if !(2000..=2099).contains(&date.year()) {
        return Err(Error::Invalid(
            "Rok raportu musi należeć do zakresu 2000–2099.".to_owned(),
        ));
    }
    Ok(format!(
        "{:02}{:02}{:02}",
        date.year() - 2000,
        date.month(),
        date.day()
    ))
}
fn read_number(session: &mut Session, command: &'static str, tag: char) -> Result<String, Error> {
    let body = query(session, command, command)?;
    let value = field(&body, tag)?;
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(frame::corrupt());
    }
    Ok(value.to_owned())
}
fn dispatch(session: &mut Session, steps: &[Step], journal: &mut dyn Journal) -> Result<(), Error> {
    for (index, step) in steps.iter().enumerate() {
        if let Err(error) = journal.dispatching(step.command) {
            let error = Error::Journal(error);
            return Err(if index == 0 { error } else { uncertain(error) });
        }
        if let Err(error) = session.command(step.command, &step.body, PRINT_TIMEOUT) {
            let certain = index == 0
                && step.command == "LOR"
                && matches!(error, Error::Device { rejected: true, .. });
            return Err(if certain { error } else { uncertain(error) });
        }
    }
    Ok(())
}
fn uncertain(error: Error) -> Error {
    Error::Uncertain(Box::new(error))
}
fn field(body: &str, tag: char) -> Result<&str, Error> {
    unique(body.split('\u{1d}').skip(1), tag)
}
fn subfield(body: &str, tag: char) -> Result<&str, Error> {
    unique(body.split('\u{1e}'), tag)
}
fn unique<'a>(fields: impl Iterator<Item = &'a str>, tag: char) -> Result<&'a str, Error> {
    let mut values = fields.filter_map(|f| f.strip_prefix(tag));
    let value = values.next().ok_or_else(frame::corrupt)?;
    if values.next().is_some() {
        return Err(frame::corrupt());
    }
    Ok(value)
}
