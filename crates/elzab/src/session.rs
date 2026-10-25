//! B:156-254,276-298,342-390: fresh IDs, validated correlation, no retransmission.
//! A damaged or missing response poisons the connection; no later write is allowed.
use crate::{frame, text};
use fiscal::{
    cp1250,
    error::Error,
    link::{Connection, Link, Port},
};
use std::{
    fmt, io,
    time::{Duration, Instant},
};

pub struct Session {
    port: Box<dyn Port>,
    next_id: u32,
    poisoned: bool,
    pending: Vec<u8>,
    scale: u32,
}
impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("next_id", &self.next_id)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}
impl Session {
    pub fn open(link: &dyn Link, connection: &Connection) -> Result<Self, Error> {
        #[cfg(any(test, feature = "simulator"))]
        let scale = link.time_scale().max(1);
        #[cfg(not(any(test, feature = "simulator")))]
        let scale = 1;
        Ok(Self {
            port: link.open(&connection.port, connection.baud_rate)?,
            next_id: 1,
            poisoned: false,
            pending: Vec::with_capacity(frame::MAX_FRAME),
            scale,
        })
    }

    pub fn command(
        &mut self,
        command: &'static str,
        body: &str,
        timeout: Duration,
    ) -> Result<String, Error> {
        if self.poisoned {
            return Err(Error::NotReady(
                "Poprzednia odpowiedź Elzab jest niepewna. Zamknij sesję.".to_owned(),
            ));
        }
        if self.next_id > 999_999_999 {
            return Err(frame::corrupt());
        }
        let id = self.next_id;
        let raw = frame::encode(&id.to_string(), &text::encode_body(body)?)?;
        self.next_id += 1;
        self.poisoned = true;
        let timeout = (timeout / self.scale).max(Duration::from_millis(1));
        let deadline = Instant::now() + timeout;
        self.port.write_all(&raw, timeout).map_err(Error::Io)?;
        let mut result = Vec::new();
        let mut expected_frame = 1;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout(command));
            }
            let raw = self.read_frame(command, remaining)?;
            let (header, data) = frame::decode(&raw)?;
            let header = Header::parse(header)?;
            if header.id != id {
                continue;
            }
            if header.status != "SA" {
                if expected_frame != 1
                    || header.number != 1
                    || header.maximum != 1
                    || !data.is_empty()
                {
                    return Err(frame::corrupt());
                }
                self.poisoned = false;
                return Err(Error::Device {
                    code: header.status.bytes().last().map_or(0, u32::from),
                    command: command.to_owned(),
                    detail: format!(
                        "Elzab odrzucił transmisję {command}: {}. Polecenie nie zostało ponowione.",
                        header.status
                    ),
                    rejected: true,
                });
            }
            if header.number != expected_frame
                || (header.maximum != 0 && header.maximum < header.number)
            {
                return Err(frame::corrupt());
            }
            // B:207-229: unknown total is 0 until the final numbered frame.
            if result.len() + data.len() > 65_536 {
                return Err(frame::corrupt());
            }
            result.extend_from_slice(data);
            if header.maximum == header.number {
                break;
            }
            expected_frame += 1;
        }
        let body = cp1250::decode(&result).ok_or_else(frame::corrupt)?;
        let (error, _) = body.split_once('\u{1d}').unwrap_or((&body, ""));
        if !error.is_empty() && error != "0" {
            let code = number(error)?;
            self.poisoned = false;
            return Err(Error::Device {
                code,
                command: command.to_owned(),
                detail: format!("Błąd drukarki Elzab {code} podczas {command}."),
                rejected: false,
            });
        }
        self.poisoned = false;
        Ok(body)
    }

    fn read_frame(&mut self, command: &'static str, timeout: Duration) -> Result<Vec<u8>, Error> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.pending.first().is_some_and(|b| *b != frame::STX) {
                return Err(frame::corrupt());
            }
            if let Some(end) = self.pending.iter().position(|b| *b == frame::ETX) {
                let length = end + 1;
                return Ok(self.pending.drain(..length).collect());
            }
            if self.pending.len() >= frame::MAX_FRAME {
                return Err(frame::corrupt());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout(command));
            }
            let mut bytes = [0; frame::MAX_FRAME];
            let length = self.port.read(&mut bytes, remaining).map_err(|error| {
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) {
                    Error::Timeout(command)
                } else {
                    Error::Io(error)
                }
            })?;
            if length == 0 {
                return Err(Error::Io(io::ErrorKind::UnexpectedEof.into()));
            }
            self.pending
                .extend_from_slice(bytes.get(..length).ok_or_else(frame::corrupt)?);
        }
    }
}

#[derive(Debug)]
struct Header<'a> {
    id: u32,
    status: &'a str,
    number: u32,
    maximum: u32,
}
impl<'a> Header<'a> {
    fn parse(header: &'a str) -> Result<Self, Error> {
        let mut fields = header.split('\u{1d}');
        let id = number(fields.next().ok_or_else(frame::corrupt)?)?;
        let status = fields.next().filter(|s| !s.is_empty()).unwrap_or("SA");
        let number = number(fields.next().filter(|s| !s.is_empty()).unwrap_or("1"))?;
        let maximum = self::number(fields.next().filter(|s| !s.is_empty()).unwrap_or("1"))?;
        let transform = fields.next().filter(|s| !s.is_empty()).unwrap_or("NDT");
        if fields.next().is_some() {
            return Err(frame::corrupt());
        }
        if !matches!(
            status,
            "SA" | "SB"
                | "SC"
                | "SE"
                | "SF"
                | "SG"
                | "SH"
                | "SI"
                | "SJ"
                | "SK"
                | "SL"
                | "SM"
                | "SN"
        ) || transform != "NDT"
        {
            return Err(frame::corrupt());
        }
        if number == 0 {
            return Err(frame::corrupt());
        }
        Ok(Self {
            id,
            status,
            number,
            maximum,
        })
    }
}
fn number(value: &str) -> Result<u32, Error> {
    if value.is_empty() || value.len() > 9 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(frame::corrupt());
    }
    value.parse().map_err(|_number| frame::corrupt())
}

#[cfg(test)]
mod tests;
