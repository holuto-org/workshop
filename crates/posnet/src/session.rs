//! One exclusive connection to the printer. No command is ever retried automatically.

use std::{
    fmt, io,
    sync::atomic::{AtomicU16, Ordering},
    time::{Duration, Instant},
};

use serialport::{SerialPort, SerialPortInfo, SerialPortType};

use crate::{
    Error,
    frame::{self, ETX, MAX_FRAME, Reply, STX},
};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
const READ_CHUNK: usize = 256;

pub trait Port: Send {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()>;
    /// Returns [`io::ErrorKind::TimedOut`] if no bytes arrive within `timeout`.
    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize>;
    /// Explicit simulator holds synchronize threads, rather than testing timeout expiry.
    #[cfg(any(test, feature = "simulator"))]
    fn operator_wait_limit(&self, scaled: Duration) -> Duration {
        scaled
    }
}

pub trait Link: Send + Sync + fmt::Debug {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error>;
    /// Probe order, likeliest ports first.
    fn candidates(&self) -> Vec<String>;
    /// Simulator-only time scaling; serial links retain all production deadlines.
    #[cfg(any(test, feature = "simulator"))]
    fn time_scale(&self) -> u32 {
        1
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub port: String,
    pub baud_rate: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SerialLink;

impl Link for SerialLink {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        let opened = serialport::new(port, baud_rate)
            .timeout(DEFAULT_TIMEOUT)
            .open_native()
            .map_err(|source| Error::Open {
                port: port.to_owned(),
                source: source.into(),
            })?;
        Ok(Box::new(SerialStream(opened)))
    }

    fn candidates(&self) -> Vec<String> {
        serial_ports()
    }
}

/// USB ports first. On macOS, use `/dev/cu.*`: duplicate `/dev/tty.*` nodes wait for a carrier.
pub fn serial_ports() -> Vec<String> {
    filter_serial_ports(
        serialport::available_ports().unwrap_or_default(),
        cfg!(target_os = "macos"),
    )
}

fn filter_serial_ports(mut ports: Vec<SerialPortInfo>, macos: bool) -> Vec<String> {
    ports.retain(|port| !macos || !port.port_name.starts_with("/dev/tty."));
    ports.sort_by_key(|port| !matches!(port.port_type, SerialPortType::UsbPort(_)));
    ports.into_iter().map(|port| port.port_name).collect()
}

/// Native port avoids nesting trait objects.
pub(crate) struct SerialStream<P>(pub(crate) P);

impl<P: SerialPort> Port for SerialStream<P> {
    /// No flush: `tcdrain`/`FlushFileBuffers` can block indefinitely; the reply confirms delivery.
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        self.0.set_timeout(timeout)?;
        self.0.write_all(bytes)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.0.set_timeout(timeout)?;
        match self.0.read(buffer)? {
            0 => Err(io::ErrorKind::TimedOut.into()),
            read => Ok(read),
        }
    }
}

pub struct Session {
    port: Box<dyn Port>,
    /// Bytes beyond the last frame, retained for the next reply.
    pending: Vec<u8>,
    #[cfg(any(test, feature = "simulator"))]
    time_scale: u32,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn open(link: &dyn Link, connection: &Connection) -> Result<Self, Error> {
        if connection.port.trim().is_empty() {
            return Err(Error::NoPort);
        }
        let session = Self::new(link.open(&connection.port, connection.baud_rate)?);
        #[cfg(any(test, feature = "simulator"))]
        let session = Self {
            time_scale: link.time_scale().max(1),
            ..session
        };
        Ok(session)
    }

    pub(crate) fn new(port: Box<dyn Port>) -> Self {
        Self {
            port,
            pending: Vec::new(),
            #[cfg(any(test, feature = "simulator"))]
            time_scale: 1,
        }
    }

    #[cfg(any(test, feature = "simulator"))]
    pub(crate) fn timeout(&self, duration: Duration) -> Duration {
        if self.time_scale == 1 {
            duration
        } else {
            (duration / self.time_scale).max(Duration::from_millis(100))
        }
    }

    #[cfg(not(any(test, feature = "simulator")))]
    #[expect(
        clippy::unused_self,
        reason = "production keeps the session API without a timing override"
    )]
    pub(crate) const fn timeout(&self, duration: Duration) -> Duration {
        duration
    }

    #[cfg(any(test, feature = "simulator"))]
    pub(crate) fn operator_wait_limit(&self, duration: Duration) -> Duration {
        self.port.operator_wait_limit(self.timeout(duration))
    }

    #[cfg(not(any(test, feature = "simulator")))]
    #[expect(
        clippy::unused_self,
        reason = "production keeps the session API without a timing override"
    )]
    pub(crate) const fn operator_wait_limit(&self, duration: Duration) -> Duration {
        duration
    }

    /// Sends once and waits up to `timeout`.
    pub fn command(
        &mut self,
        command: &'static str,
        params: &[(&str, String)],
        timeout: Duration,
    ) -> Result<Reply, Error> {
        let deadline = self.send(command, params, timeout)?;
        self.receive(command, deadline)
    }

    /// A late reply to another command is an error, not this command's success.
    pub(crate) fn receive(
        &mut self,
        command: &'static str,
        deadline: Instant,
    ) -> Result<Reply, Error> {
        loop {
            let (answered, reply) = self.next_frame(command, deadline)?;
            // Tokens mark replies to earlier asynchronous status queries.
            if reply.get("@").is_some() {
                continue;
            }
            let reply = reply.into_result(&answered)?;
            if answered != command {
                return Err(Error::Protocol(format!(
                    "Oczekiwano odpowiedzi {command}, otrzymano {answered}."
                )));
            }
            return Ok(reply);
        }
    }

    /// Returns the reply deadline after sending once.
    pub(crate) fn send(
        &mut self,
        command: &'static str,
        params: &[(&str, String)],
        timeout: Duration,
    ) -> Result<Instant, Error> {
        let timeout = self.timeout(timeout);
        tracing::debug!(
            command,
            timeout_ms = timeout.as_millis(),
            "Sending printer command once"
        );
        let frame = frame::encode(command, params)?;
        let deadline = Instant::now() + timeout;
        self.port
            .write_all(&frame, timeout)
            .map_err(|error| io_error(command, error))?;
        Ok(deadline)
    }

    /// Accepts any reply envelope, retaining error codes and query tokens for correlation.
    /// Silence until `deadline` times out `command`.
    pub(crate) fn next_frame(
        &mut self,
        command: &'static str,
        deadline: Instant,
    ) -> Result<(String, Reply), Error> {
        let mut chunk = [0_u8; READ_CHUNK];
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == ETX) {
                let raw: Vec<u8> = self.pending.drain(..=end).collect();
                // Discard line noise before the last STX; CRC validates the frame.
                let Some(start) = raw.iter().rposition(|byte| *byte == STX) else {
                    continue;
                };
                return frame::decode_response(raw.get(start..).unwrap_or_default());
            }
            if self.pending.len() >= MAX_FRAME {
                return Err(Error::Protocol("Zbyt długa odpowiedź drukarki.".to_owned()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout(command));
            }
            let read = match self.port.read(&mut chunk, remaining) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => result.map_err(|error| io_error(command, error))?,
            };
            self.pending
                .extend_from_slice(chunk.get(..read).unwrap_or_default());
        }
    }
}

/// Echoed request token, `0000`–`9999` (spec p. 11).
pub(crate) fn token() -> String {
    static NEXT: AtomicU16 = AtomicU16::new(0);
    next_token(&NEXT)
}

fn next_token(next: &AtomicU16) -> String {
    let value = next
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some((value + 1) % 10_000)
        })
        .unwrap_or_else(|value| value);
    format!("{value:04}")
}

fn io_error(command: &'static str, error: io::Error) -> Error {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        Error::Timeout(command)
    } else {
        Error::Io(error)
    }
}

#[cfg(test)]
mod tests;
