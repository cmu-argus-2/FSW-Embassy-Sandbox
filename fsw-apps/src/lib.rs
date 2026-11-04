//! Argus mission logic. Reaches hardware only through the traits in `fsw-hal`.
//!
//! - [`adcs`]: attitude determination and control logic.
//! - [`eps`]: electrical power subsystem logic.
//! - [`tasks`]: the loops that run each app.
#![no_std]

pub mod adcs;
pub mod eps;
pub mod tasks;
