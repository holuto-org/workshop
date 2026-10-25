//! Posnet Temo Online driver over a unique_number port.
//!
//! Each operation opens an exclusive session and verifies identity, fiscal mode, header,
//! mechanism, command queue, transaction state and VAT table before fiscal commands.
//! Commands are never retried. A lost reply leaves an uncertain outcome for the caller to resolve.
//! The caller's [`Journal`](fiscal::journal::Journal) persists intent before each command,
//! surviving restarts.

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
