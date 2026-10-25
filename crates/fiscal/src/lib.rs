//! Shared fiscal values, durable-intent contract and printer connections.

#![forbid(unsafe_code)]

#[cfg(all(workshop_acceptance, not(debug_assertions)))]
compile_error!("workshop_acceptance is restricted to local debug builds.");

pub mod cp1250;
pub mod decimal;
pub mod device;
pub mod driver;
pub mod error;
pub mod journal;
pub mod link;
pub mod money;
pub mod receipt;
pub mod report;
