//! Attitude determination and control. Ported from FSW-mainboard `flight/apps/adcs/`, one file
//! per Python file. Everything here is pure: values in, values out, no I/O or timing. Hardware is
//! reached by the task, through the `fsw-hal` traits.
//!
//! - [`sensors`]: reading checks (`sensors.py`).
//!
//! This file holds only the module list and the types shared across files.

pub mod sensors;

use fsw_hal::Vec3;

/// Sensor status. Discriminants match `StatusConst` in `consts.py` so telemetry stays compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum Status {
    Ok = 0,
    GyroFail = 21,
}

/// A checked sensor reading. Like the Python code, a failed reading carries a zero vector.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Reading {
    pub status: Status,
    pub value: Vec3,
}
