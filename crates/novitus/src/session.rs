//! One exclusive native connection.
//!
//! Mode 3 is sent once without waiting for its undocumented initialization ACK.
//! Correlates ACKs (including four-digit errors) and parses raw status alongside
//! fragmented frames.

use crate::frame;
use fiscal::{
    cp1250,
    error::Error,
    link::{Connection, Link, Port},
};
use std::{
    collections::VecDeque,
    fmt, io,
    time::{Duration, Instant},
};

pub const DLE: u8 = 0x10;
pub const ENQ: u8 = 0x05;
pub const DLE2: u8 = 0x1A;
const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub body: String,
    /// Validated configuration pairs; empty for other reply schemas.
    pub options: Vec<(u32, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DleStatus {
    pub online: bool,
    pub paper_or_battery: bool,
    pub mechanism_error: bool,
}
impl DleStatus {
    pub fn decode(byte: u8) -> Result<Self, Error> {
        if !status_matches(DLE, byte) {
            return Err(frame::corrupt());
        }
        Ok(Self {
            online: byte & 4 != 0,
            paper_or_battery: byte & 2 != 0,
            mechanism_error: byte & 1 != 0,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnqStatus {
    pub fiscal: bool,
    pub transaction: bool,
}
impl EnqStatus {
    pub fn decode(byte: u8) -> Result<Self, Error> {
        if !status_matches(ENQ, byte) {
            return Err(frame::corrupt());
        }
        Ok(Self {
            fiscal: byte & 8 != 0,
            transaction: byte & 2 != 0,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtendedStatus {
    pub last_error: u32,
    /// DLE2 bytes in arrival order; mapping to the manual's byte labels is unverified.
    pub flags_wire: [u8; 4],
}
impl ExtendedStatus {
    pub fn parse(body: &str) -> Result<Self, Error> {
        let mut fields = body
            .strip_prefix("@s")
            .ok_or_else(frame::corrupt)?
            .split(';');
        let last_error = decimal(fields.next())?;
        // Last printout and receipt numbers: validated, unused.
        let _last_print = decimal(fields.next())?;
        let _last_receipt = decimal(fields.next())?;
        let flags_wire = fields
            .next()
            .filter(|flags| flags.len() == 8 && flags.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .and_then(|flags| u32::from_str_radix(flags, 16).ok())
            .ok_or_else(frame::corrupt)?
            .to_be_bytes();
        if fields.next() != Some("") || fields.next().is_some() {
            return Err(frame::corrupt());
        }
        Ok(Self {
            last_error,
            flags_wire,
        })
    }
}

#[derive(Debug)]
enum Event {
    Frame(Vec<u8>),
    Status(u8),
}

pub struct Session {
    port: Box<dyn Port>,
    pending: Vec<u8>,
    queued: VecDeque<Event>,
    initialization_ack: bool,
    discovery: bool,
    discovery_ack: Option<&'static str>,
    blocked: bool,
    #[cfg(any(test, feature = "simulator"))]
    time_scale: u32,
}
impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("pending", &self.pending.len())
            .field("blocked", &self.blocked)
            .finish_non_exhaustive()
    }
}
impl Session {
    pub fn open(link: &dyn Link, connection: &Connection) -> Result<Self, Error> {
        let mut session = Self::open_discovery(link, connection)?;
        session.initialization_ack = true;
        session.discovery = false;
        let timeout = session.timeout(TIMEOUT);
        session
            .port
            .write_all(&frame::encode(b"3#e"), timeout)
            .map_err(io_error)?;
        Ok(session)
    }

    /// Discovery must not configure an unidentified device's error mode.
    pub(crate) fn open_discovery(link: &dyn Link, connection: &Connection) -> Result<Self, Error> {
        if connection.port.trim().is_empty() {
            return Err(Error::NoPort);
        }
        Ok(Self {
            port: link.open(&connection.port, connection.baud_rate)?,
            pending: Vec::new(),
            queued: VecDeque::new(),
            initialization_ack: false,
            discovery: true,
            discovery_ack: None,
            blocked: false,
            #[cfg(any(test, feature = "simulator"))]
            time_scale: link.time_scale().max(1),
        })
    }

    #[cfg(any(test, feature = "simulator"))]
    pub fn timeout(&self, duration: Duration) -> Duration {
        if self.time_scale == 1 {
            duration
        } else {
            (duration / self.time_scale).max(Duration::from_millis(100))
        }
    }

    #[cfg(not(any(test, feature = "simulator")))]
    #[expect(clippy::unused_self, reason = "production has no timing override")]
    pub const fn timeout(&self, duration: Duration) -> Duration {
        duration
    }

    #[cfg(any(test, feature = "simulator"))]
    pub fn operator_wait_limit(&self, duration: Duration) -> Duration {
        self.port.operator_wait_limit(self.timeout(duration))
    }

    #[cfg(not(any(test, feature = "simulator")))]
    #[expect(clippy::unused_self, reason = "production has no timing override")]
    pub const fn operator_wait_limit(&self, duration: Duration) -> Duration {
        duration
    }

    /// Returns data and, where required, a matching mode-3 completion.
    /// Uncertain replies block new commands, but permit raw status reads.
    pub fn command(&mut self, body: &str, timeout: Duration) -> Result<Reply, Error> {
        if self.blocked {
            return Err(Error::Protocol(
                "Sesja Novitus ma nierozstrzygnięte polecenie.".to_owned(),
            ));
        }
        let command = identifier(body)
            .ok_or_else(|| Error::Invalid("Nieprawidłowe polecenie Novitus.".to_owned()))?;
        let bytes = encode_body(body)?;
        let raw = if no_request_checksum(body) {
            frame::query(&bytes)
        } else {
            frame::encode(&bytes)
        };
        if raw.len() > frame::MAX_FRAME {
            return Err(Error::Invalid(
                "Polecenie Novitus jest zbyt długie.".to_owned(),
            ));
        }
        self.blocked = true;
        let timeout = self.timeout(timeout);
        tracing::debug!(
            command,
            timeout_ms = timeout.as_millis(),
            "Sending printer command once"
        );
        let deadline = Instant::now() + timeout;
        self.port.write_all(&raw, timeout).map_err(io_error)?;
        let result = self.receive(body, command, deadline);
        if result.is_ok() || matches!(result, Err(Error::Device { .. })) {
            self.blocked = false;
        }
        result
    }

    /// Receive the original command's ACK without resending.
    /// Fiscal schemas are ACK-only, so no partial data is discarded.
    pub(crate) fn finish_fiscal(&mut self, command: &str, deadline: Instant) -> Result<(), Error> {
        if !self.blocked || !is_fiscal(command) {
            return Err(frame::corrupt());
        }
        let result = self.receive(command, command, deadline).map(|_reply| ());
        if result.is_ok() || matches!(result, Err(Error::Device { .. })) {
            self.blocked = false;
        }
        result
    }

    fn receive(&mut self, request: &str, command: &str, deadline: Instant) -> Result<Reply, Error> {
        let mut data = None;
        let mut completed = false;
        let expected = response_prefix(request);
        loop {
            let queued = self.queued.iter().position(|event| {
                let Event::Frame(raw) = event else {
                    return false;
                };
                frame::unframe(raw).is_ok_and(|body| !body.starts_with(b"@s"))
            });
            let event =
                if let Some(event) = queued.and_then(|position| self.queued.remove(position)) {
                    event
                } else {
                    self.next_event(deadline)?
                };
            let Event::Frame(raw) = event else {
                self.enqueue(event)?;
                continue;
            };
            let payload = frame::unframe(&raw)?;
            if let Some((code, answered)) = acknowledgment(payload)? {
                if self.initialization_ack && answered == "#e" {
                    self.initialization_ack = false;
                    device_result(code, answered)?;
                    continue;
                }
                if answered != command {
                    return Err(Error::Protocol(format!(
                        "Oczekiwano ACK {command}, otrzymano {answered}."
                    )));
                }
                device_result(code, answered)?;
                completed = true;
                if expected.is_none() {
                    return Ok(Reply {
                        body: decode_body(payload)?,
                        options: Vec::new(),
                    });
                }
            } else if payload.starts_with(b"@s") {
                let body = decode_body(frame::decode(&raw)?)?;
                let _status = ExtendedStatus::parse(&body)?;
                self.enqueue(Event::Frame(raw))?;
                continue;
            } else {
                let Some((prefix, checksum, _ack)) = expected.as_ref() else {
                    return Err(frame::corrupt());
                };
                let body = decode_body(if *checksum {
                    frame::decode(&raw)?
                } else {
                    payload
                })?;
                if data.is_some() {
                    return Err(frame::corrupt());
                }
                let options = if command == "^c" {
                    configuration_options(&body)?
                } else {
                    if !body.starts_with(prefix) {
                        return Err(frame::corrupt());
                    }
                    Vec::new()
                };
                data = Some(Reply { body, options });
            }
            if let Some((_, _, ack)) = &expected
                && (self.discovery || !ack || completed)
                && let Some(reply) = data.take()
            {
                // Mode 0 never sends this ACK; mode 3 sends it after the data.
                if self.discovery && !completed && *ack {
                    self.discovery_ack = query_identifier(command);
                }
                return Ok(reply);
            }
        }
    }

    /// Ends discovery without leaving an optional query ACK unread for the next session:
    /// `#c` never sends one, and the printer answers in order, so its reply follows any
    /// pending ACK. A blocked session has an unanswered command and is left as is.
    pub(crate) fn settle_discovery(&mut self) -> Result<(), Error> {
        if self.blocked || self.discovery_ack.is_none() {
            return Ok(());
        }
        self.command("#c", TIMEOUT).map(|_reply| ())
    }

    /// DLE and ENQ have single-byte replies; DLE2 uses `extended_status`.
    pub fn realtime(&mut self, byte: u8) -> Result<u8, Error> {
        self.realtime_until(byte, Instant::now() + self.timeout(TIMEOUT))
    }

    pub(crate) fn realtime_until(&mut self, byte: u8, cap: Instant) -> Result<u8, Error> {
        if !matches!(byte, DLE | ENQ) {
            return Err(Error::Invalid("Zapytanie wymaga DLE lub ENQ.".to_owned()));
        }
        let deadline = self.send_realtime(byte, cap)?;
        loop {
            match self.next_event(deadline)? {
                Event::Status(status) if status_matches(byte, status) => return Ok(status),
                event @ (Event::Status(_) | Event::Frame(_)) => self.enqueue(event)?,
            }
        }
    }

    pub fn extended_status(&mut self) -> Result<ExtendedStatus, Error> {
        self.extended_status_until(Instant::now() + self.timeout(TIMEOUT))
    }

    pub(crate) fn extended_status_until(&mut self, cap: Instant) -> Result<ExtendedStatus, Error> {
        let deadline = self.send_realtime(DLE2, cap)?;
        loop {
            match self.next_event(deadline)? {
                Event::Frame(raw) if frame::unframe(&raw)?.starts_with(b"@s") => {
                    return ExtendedStatus::parse(&decode_body(frame::decode(&raw)?)?);
                },
                event @ (Event::Frame(_) | Event::Status(_)) => self.enqueue(event)?,
            }
        }
    }

    fn send_realtime(&mut self, byte: u8, cap: Instant) -> Result<Instant, Error> {
        let deadline = (Instant::now() + self.timeout(TIMEOUT)).min(cap);
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::Timeout("Novitus"));
        }
        self.port.write_all(&[byte], remaining).map_err(io_error)?;
        Ok(deadline)
    }

    fn enqueue(&mut self, event: Event) -> Result<(), Error> {
        if let Event::Status(status) = &event
            && !status_matches(DLE, *status)
            && !status_matches(ENQ, *status)
        {
            return Err(frame::corrupt());
        }
        if self.queued.len() >= 64 {
            return Err(frame::corrupt());
        }
        self.queued.push_back(event);
        Ok(())
    }

    fn next_event(&mut self, deadline: Instant) -> Result<Event, Error> {
        let mut chunk = [0; 256];
        loop {
            if self.pending.first() == Some(&0x1B) {
                if self.pending.len() >= 2 && !self.pending.starts_with(&frame::START) {
                    return Err(frame::corrupt());
                }
                if let Some(end) = self.pending.windows(2).position(|pair| pair == frame::END) {
                    let raw: Vec<_> = self.pending.drain(..end + 2).collect();
                    if let Some(expected) = self.discovery_ack
                        && let Some((code, answered)) = acknowledgment(frame::unframe(&raw)?)?
                        && answered == expected
                    {
                        self.discovery_ack = None;
                        device_result(code, answered)?;
                        continue;
                    }
                    return Ok(Event::Frame(raw));
                }
            } else if !self.pending.is_empty() {
                return Ok(Event::Status(self.pending.remove(0)));
            }
            if self.pending.len() >= frame::MAX_FRAME {
                return Err(frame::corrupt());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout("Novitus"));
            }
            match self.port.read(&mut chunk, remaining) {
                Ok(0) => return Err(Error::Io(io::ErrorKind::UnexpectedEof.into())),
                Ok(read) => self
                    .pending
                    .extend_from_slice(chunk.get(..read).ok_or_else(frame::corrupt)?),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
                Err(error) => return Err(io_error(error)),
            }
        }
    }
}

const fn status_matches(request: u8, status: u8) -> bool {
    if request == DLE {
        status & 0xF8 == 0x70
    } else {
        status & 0xF0 == 0x60
    }
}

/// Fiscal schemas reply with an ACK only.
pub(crate) fn is_fiscal(command: &str) -> bool {
    matches!(
        command,
        "$h" | "$l" | "$z" | "$b" | "$y" | "@z" | "#r" | "#o"
    )
}

fn decimal(value: Option<&str>) -> Result<u32, Error> {
    value
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse().ok())
        .ok_or_else(frame::corrupt)
}
fn acknowledgment(body: &[u8]) -> Result<Option<(u32, &str)>, Error> {
    let digits = body.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if body
        .get(digits..)
        .is_none_or(|rest| !rest.starts_with(b"#Z"))
    {
        return Ok(None);
    }
    // ACK fields are ASCII decimal and command syntax, never wire text.
    let body = str::from_utf8(body)
        .map_err(|error| Error::Protocol(format!("Nieprawidłowy ACK Novitus: {error}.")))?;
    let (code, command) = body.split_once("#Z").ok_or_else(frame::corrupt)?;
    if identifier(command) != Some(command) {
        return Err(frame::corrupt());
    }
    Ok(Some((decimal(Some(code))?, command)))
}
fn device_result(code: u32, command: &str) -> Result<(), Error> {
    if code == 0 {
        return Ok(());
    }
    Err(Error::Device {
        code,
        command: command.to_owned(),
        detail: format!(
            "Drukarka Novitus zwróciła błąd {code} ({command}).{}",
            explanation(command, code)
        ),
        rejected: false,
    })
}
/// Refusals of the commands sent here; codes are numbered per command.
fn explanation(command: &str, code: u32) -> &'static str {
    match (command, code) {
        ("$h" | "#r" | "#o", 1) => " Nie zainicjowano zegara drukarki.",
        ("$h" | "#r" | "#o", 6) => " Błąd pamięci fiskalnej. Wezwij serwis.",
        ("$h", 40) => " Brak nagłówka w pamięci drukarki.",
        ("$h", 94) => " Sprzedaż zablokowana: założona zwora serwisowa.",
        ("#r", 7) => {
            " Data drukarki jest wcześniejsza niż ostatni zapis albo zmieniła się przed raportem."
        },
        ("#r", 36) => " Raport dobowy bez sprzedaży jest już zapisany z tą datą.",
        ("#r", 37) => " Raport przerwano na klawiaturze drukarki.",
        ("#o", 4) => {
            " Nieprawidłowy zakres dat lub brak zapisów w pamięci fiskalnej dla tego zakresu."
        },
        _ => "",
    }
}

pub(crate) fn identifier(body: &str) -> Option<&str> {
    let start = body.find(['#', '$', '^', '@', '?'])?;
    let rest = body.get(start..)?;
    // Text may follow these identifiers without a separator; do not parse it as command letters.
    if let Some(command) = rest.get(..2)
        && matches!(command, "$l" | "$h" | "$b" | "$y" | "$z" | "@z")
    {
        return Some(command);
    }
    let length = 1 + rest
        .bytes()
        .skip(1)
        .take_while(u8::is_ascii_alphabetic)
        .count();
    (2..=5)
        .contains(&length)
        .then(|| rest.get(..length))
        .flatten()
}

fn query_identifier(command: &str) -> Option<&'static str> {
    ["#v", "$i", "^c", "^u", "#c", "#s", "#n", "?l"]
        .into_iter()
        .find(|query| *query == command)
}
pub(crate) fn no_request_checksum(body: &str) -> bool {
    matches!(identifier(body), Some("#v" | "#c" | "#n" | "$i")) || body == "23#s"
}
fn response_prefix(body: &str) -> Option<(String, bool, bool)> {
    match identifier(body)? {
        "#v" => Some(("1#R".to_owned(), false, false)),
        "#c" => Some(("1#C".to_owned(), false, false)),
        "#n" => Some(("1#E".to_owned(), false, false)),
        "#s" if body == "23#s" => Some(("2#X".to_owned(), true, false)),
        "$i" => Some((format!("{}$I", body.split('$').next()?), true, true)),
        "^u" => Some(("1#U".to_owned(), true, true)),
        "^c" => Some(("^c".to_owned(), true, true)),
        "?l" => Some(("0?l".to_owned(), true, true)),
        _ => None,
    }
}

/// Counted option/value pairs.
fn configuration_options(body: &str) -> Result<Vec<(u32, String)>, Error> {
    let (count, fields) = body.split_once("^c").ok_or_else(frame::corrupt)?;
    let count = decimal(Some(count))?;
    let mut fields = fields.split('\r');
    let mut options = Vec::new();
    for _ in 0..count {
        let option = decimal(fields.next())?;
        let value = fields.next().ok_or_else(frame::corrupt)?;
        options.push((option, value.to_owned()));
    }
    if fields.next() != Some("") || fields.next().is_some() {
        return Err(frame::corrupt());
    }
    Ok(options)
}
pub(crate) fn encode_body(body: &str) -> Result<Vec<u8>, Error> {
    if body
        .chars()
        .any(|character| character.is_control() && character != '\r')
    {
        return Err(Error::Invalid(
            "Niedozwolony znak sterujący w poleceniu Novitus.".to_owned(),
        ));
    }
    cp1250::encode(body)
        .ok_or_else(|| Error::Invalid("Tekst nie mieści się w Windows-1250.".to_owned()))
}
fn decode_body(body: &[u8]) -> Result<String, Error> {
    let text = cp1250::decode(body).ok_or_else(frame::corrupt)?;
    if text
        .chars()
        .any(|character| character.is_control() && character != '\r')
    {
        return Err(frame::corrupt());
    }
    Ok(text)
}
fn io_error(error: io::Error) -> Error {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        Error::Timeout("Novitus")
    } else {
        Error::Io(error)
    }
}

#[cfg(test)]
mod tests;
