//! Native Novitus ESC-P transport. No command is automatically retried.

pub mod frame;
pub mod printer;
pub mod profile;
pub mod receipt;
pub mod session;
pub mod text;

#[cfg(any(test, feature = "simulator"))]
pub mod simulator;
