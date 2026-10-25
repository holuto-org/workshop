//! Posnet Temo Online driver over a serial port.
//!
//! Each operation verifies identity, fiscal mode, header, mechanism, command queue,
//! transaction state, VAT table and Windows-1250 PC interfaces in an exclusive session
//! before fiscal commands.
//! Commands are never retried; a lost reply leaves the outcome uncertain.
//! The caller's [`Journal`](fiscal::journal::Journal) persists intent before each command.

#![forbid(unsafe_code)]

pub mod capabilities;
pub mod frame;
pub mod printer;
pub mod receipt;
pub mod session;
#[cfg(any(test, feature = "simulator"))]
pub mod simulator;
pub mod text;

#[cfg(test)]
mod testing;

#[cfg(test)]
mod tests;
