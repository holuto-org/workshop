//! Exclusive serial connections shared by fiscal printer drivers.

use std::{fmt, io, time::Duration};

use serde::Serialize;
use serialport::{SerialPort, SerialPortInfo, SerialPortType};

use crate::{device::Vendor, error::Error};

#[cfg(target_os = "linux")]
mod linux;

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
    fn candidates(&self) -> Vec<Candidate>;
    /// Simulator-only time scaling; serial links retain all production deadlines.
    #[cfg(any(test, feature = "simulator"))]
    fn time_scale(&self) -> u32 {
        1
    }
}

/// Passive transport metadata, not a verified printer identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub port: String,
    pub hint: Option<Vendor>,
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

    fn candidates(&self) -> Vec<Candidate> {
        serial_ports()
    }
}

/// Hinted USB ports, bridges, then native candidates; names are sorted within each group.
/// Enumeration never opens a port. Hints do not authorize fiscal operations.
pub fn serial_ports() -> Vec<Candidate> {
    let os = if cfg!(target_os = "linux") {
        Os::Linux
    } else if cfg!(target_os = "macos") {
        Os::Macos
    } else {
        Os::Windows
    };
    #[cfg(target_os = "linux")]
    let uart_type = linux::uart_type;
    #[cfg(not(target_os = "linux"))]
    let uart_type = |_: &str| None;
    filter(
        serialport::available_ports().unwrap_or_default(),
        os,
        &uart_type,
    )
}

#[derive(Clone, Copy)]
enum Os {
    Linux,
    Windows,
    Macos,
}

fn filter(
    ports: Vec<SerialPortInfo>,
    os: Os,
    uart_type: &dyn Fn(&str) -> Option<u32>,
) -> Vec<Candidate> {
    let mut ranked: Vec<_> = ports
        .into_iter()
        .filter_map(|port| {
            if matches!(os, Os::Macos) && port.port_name.starts_with("/dev/tty.") {
                return None;
            }
            let (rank, hint) = match port.port_type {
                SerialPortType::UsbPort(usb) => match (usb.vid, usb.pid) {
                    (0x1424, _) => (0, Some(Vendor::Posnet)),
                    (0x1A28, 0x2512) => (0, Some(Vendor::Novitus)),
                    (0x0483, 0x5740) => (0, Some(Vendor::Elzab)),
                    (0x0403 | 0x067B | 0x1A86 | 0x10C4, _) => (1, None),
                    _ => return None,
                },
                SerialPortType::BluetoothPort => return None,
                SerialPortType::PciPort | SerialPortType::Unknown => {
                    let keep = match os {
                        Os::Linux => uart_type(&port.port_name) != Some(0),
                        Os::Windows => matches!(port.port_type, SerialPortType::Unknown),
                        Os::Macos => matches!(port.port_type, SerialPortType::PciPort),
                    };
                    if !keep {
                        return None;
                    }
                    (2, None)
                },
            };
            Some((
                rank,
                Candidate {
                    port: port.port_name,
                    hint,
                },
            ))
        })
        .collect();
    ranked.sort_by(|(a_rank, a), (b_rank, b)| a_rank.cmp(b_rank).then_with(|| a.port.cmp(&b.port)));
    ranked.into_iter().map(|(_, candidate)| candidate).collect()
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
