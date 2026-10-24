//! Exclusive serial connections shared by fiscal printer drivers.

use std::{fmt, io, time::Duration};

use serialport::{SerialPort, SerialPortInfo, SerialPortType};

use crate::error::Error;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

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

#[cfg(test)]
mod tests;
