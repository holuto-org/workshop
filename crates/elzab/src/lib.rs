//! Native `ElzabSTX` driver; no replay, configuration writes or recovery commands.
pub mod frame;
pub mod printer;
pub mod profile;
mod receipt;
pub mod session;
#[cfg(any(test, feature = "simulator"))]
pub mod simulator;
#[cfg(test)]
mod tests;
pub mod text;
