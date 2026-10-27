//! Argus mission logic. Reaches hardware only through the traits in `fsw-hal`.
//!
//! - [`adcs`]: attitude determination and control logic (from `flight/apps/adcs/`).
//! - [`tasks`]: the loops that run each app (from `flight/tasks/`).
#![no_std]

pub mod adcs;
pub mod tasks;
