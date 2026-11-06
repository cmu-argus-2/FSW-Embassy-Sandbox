//! Electrical power subsystem. Everything here is pure: values in, values out, no I/O or timing.
//! Hardware is reached by the task, through the `fsw-hal` traits.
//!
//! - [`power`]: rail power limits and the battery power flag.
//! - [`heaters`]: battery heater switching.
//!
//! This file holds only the module list and the types shared across files.

pub mod heaters;
pub mod power;

use fsw_hal::sensors::PowerReading;

/// The power rails with a monitor on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(usize)]
pub enum Rail {
    /// Mainboard input.
    Board,
    /// GPS (the "peripheral" line).
    Gps,
    /// Radio.
    Radio,
    /// Jetson payload input.
    Jetson,
}

impl Rail {
    pub const COUNT: usize = 4;
    /// Every rail, in index order.
    pub const ALL: [Rail; Self::COUNT] = [Rail::Board, Rail::Gps, Rail::Radio, Rail::Jetson];
}

/// Battery power flag. Discriminants are the telemetry codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum PowerFlag {
    /// Not yet known: no valid state of charge has been read since boot.
    Unknown = 0,
    LowPower = 1,
    Nominal = 2,
}

/// A checked rail reading. A failed read carries no value and never raises an alert.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RailReading {
    pub rail: Rail,
    /// `None` if the monitor could not be read.
    pub value: Option<PowerReading>,
    /// Average power over the alert window is at or above the rail's limit.
    pub over_limit: bool,
}
