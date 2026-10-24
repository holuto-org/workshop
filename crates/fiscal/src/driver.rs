//! The contract every fiscal printer driver implements.
//!
//! Each operation opens its own session, verifies the device before any fiscal command,
//! journals intent first and never retries or reprints; a lost reply is
//! [`Error::Uncertain`](crate::error::Error::Uncertain) for the caller to resolve.

use std::fmt;

use crate::{
    device::{Device, Vendor},
    error::Error,
    journal::{Journal, Printed},
    link::Connection,
    receipt::Receipt,
    report::Report,
};

pub trait Driver: Send + fmt::Debug {
    fn vendor(&self) -> Vendor;

    fn connection(&self) -> &Connection;

    /// Reads identity, readiness and VAT table without printing.
    fn probe(&self) -> Result<Device, Error>;

    /// Requires the paired device `serial` to be connected and ready.
    fn verify(&self, serial: &str) -> Result<Device, Error>;

    /// Rechecks identity, readiness and the VAT table `expected` before printing.
    fn print_receipt(
        &self,
        receipt: &Receipt,
        expected: &Device,
        journal: &mut dyn Journal,
    ) -> Result<Printed, Error>;

    fn print_report(
        &self,
        report: Report,
        serial: &str,
        journal: &mut dyn Journal,
    ) -> Result<(), Error>;
}
