//! Caller-owned durable fiscal intent and printed receipt evidence.

use std::io;

use jiff::Timestamp;

/// Caller-owned durable operation journal and operator-wait notifications.
pub trait Journal {
    /// Persists intent before each fiscal command; an error prevents that command's send.
    fn dispatching(&mut self, command: &'static str) -> io::Result<()>;

    /// The executing command needs operator intervention; `None` means printing resumed.
    fn waiting(&mut self, problem: Option<&str>);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printed {
    /// Post-print counter; `None` if unreadable.
    pub receipt_number: Option<String>,
    /// Printer time just before opening the receipt, to the minute; never after printing.
    pub issued_at: Timestamp,
}
