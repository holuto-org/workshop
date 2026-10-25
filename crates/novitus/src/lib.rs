//! Native Novitus ESC-P transport. No command is automatically retried.

pub mod frame;
pub mod session;
pub mod text;

#[cfg(any(test, feature = "simulator"))]
pub mod simulator;
