//! One exclusive connection to the printer. No command is ever retried automatically.

use std::{
    fmt, io,
    sync::atomic::{AtomicU16, Ordering},
    time::{Duration, Instant},
};

use serialport::{SerialPort, SerialPortType};

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
}

pub trait Link: Send + Sync + fmt::Debug {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error>;
    /// Probe order, likeliest ports first.
    fn candidates(&self) -> Vec<String>;
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
    let mut ports = serialport::available_ports().unwrap_or_default();
    ports.retain(|port| !port.port_name.starts_with("/dev/tty.") || cfg!(not(target_os = "macos")));
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
        Ok(Self::new(
            link.open(&connection.port, connection.baud_rate)?,
        ))
    }

    pub(crate) fn new(port: Box<dyn Port>) -> Self {
        Self {
            port,
            pending: Vec::new(),
        }
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
        let frame = frame::encode(command, params)?;
        let deadline = Instant::now() + timeout;
        self.port
            .write_all(&frame, timeout)
            .map_err(|error| io_error(command, error))?;
        Ok(deadline)
    }

    /// Accepts any command's reply; device errors return `Err`.
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
                return frame::decode(raw.get(start..).unwrap_or_default());
            }
            if self.pending.len() >= MAX_FRAME {
                return Err(Error::Protocol("Zbyt długa odpowiedź drukarki.".to_owned()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout(command));
            }
            let read = self
                .port
                .read(&mut chunk, remaining)
                .map_err(|error| io_error(command, error))?;
            self.pending
                .extend_from_slice(chunk.get(..read).unwrap_or_default());
        }
    }
}

/// Echoed request token, `0000`–`9999` (spec p. 11).
pub(crate) fn token() -> String {
    static NEXT: AtomicU16 = AtomicU16::new(0);
    format!("{:04}", NEXT.fetch_add(1, Ordering::Relaxed) % 10_000)
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
