//! One exclusive connection to the printer. No command is ever retried automatically.

use std::{
    fmt, io,
    time::{Duration, Instant},
};

use serialport::SerialPort;

use crate::{
    Error,
    frame::{self, ETX, MAX_FRAME, Reply, STX},
};

/// Status and setup commands answer well within this.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
const READ_CHUNK: usize = 256;

/// A byte stream to the printer.
pub trait Port: Send {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()>;
    /// Bytes that arrived within `timeout`; [`io::ErrorKind::TimedOut`] when none did.
    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize>;
}

/// Where ports come from: the OS serial ports, or the simulator in tests.
pub trait Link: Send + Sync + fmt::Debug {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error>;
    /// Ports worth probing for a printer, the likeliest first.
    fn candidates(&self) -> Vec<String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub port: String,
    pub baud_rate: u32,
}

/// The operating system's serial ports.
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

/// Serial ports a printer could be on, USB CDC ports (`ttyACM`) first. macOS lists every device
/// twice; only the call-out `/dev/cu.*` node opens without waiting for a carrier.
pub fn serial_ports() -> Vec<String> {
    let mut ports: Vec<String> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|port| port.port_name)
        .filter(|name| !name.starts_with("/dev/tty.") || cfg!(not(target_os = "macos")))
        .collect();
    ports.sort_by_key(|name| !name.contains("ttyACM"));
    ports
}

/// A native port (`TTYPort`/`COMPort`), so a session holds one trait object, not two.
pub(crate) struct SerialStream<P>(pub(crate) P);

impl<P: SerialPort> Port for SerialStream<P> {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        self.0.set_timeout(timeout)?;
        self.0.write_all(bytes)?;
        self.0.flush()
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
    /// Bytes received after the last frame; they belong to whatever reply comes next.
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

    /// Sends one command once and waits up to `timeout` for its reply. A reply to a different
    /// command (a late answer) is an error, never this command's success.
    pub fn command(
        &mut self,
        command: &'static str,
        params: &[(&str, String)],
        timeout: Duration,
    ) -> Result<Reply, Error> {
        let frame = frame::encode(command, params)?;
        let deadline = Instant::now() + timeout;
        self.port
            .write_all(&frame, timeout)
            .map_err(|error| io_error(command, error))?;
        let mut chunk = [0_u8; READ_CHUNK];
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == ETX) {
                let raw: Vec<u8> = self.pending.drain(..=end).collect();
                // Bytes before the last STX are line noise; the CRC guards the frame itself.
                let Some(start) = raw.iter().rposition(|byte| *byte == STX) else {
                    continue;
                };
                let (answered, reply) = frame::decode(raw.get(start..).unwrap_or_default())?;
                if answered != command {
                    return Err(Error::Protocol(format!(
                        "Oczekiwano odpowiedzi {command}, otrzymano {answered}."
                    )));
                }
                return Ok(reply);
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
