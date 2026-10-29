//! Attitude determination and control. Everything here is pure: values in, values out, no I/O or
//! timing. Hardware is reached by the task, through the `fsw-hal` traits.
//!
//! - [`sensors`]: reading checks.
//!
//! This file holds only the module list and the types shared across files.

pub mod sensors;

use fsw_hal::Vec3;

/// Sensor status. Discriminants are the telemetry status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum Status {
    Ok = 0,
    GyroFail = 21,
    MagFail = 31,
}

/// A checked sensor reading. A failed reading carries a zero vector.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Reading {
    pub status: Status,
    pub value: Vec3,
}

/// Magnetometer calibration, applied after converting to tesla: `(field - bias) * scale`, per axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MagCalibration {
    /// Hard-iron bias in T.
    pub bias: Vec3,
    /// Per-axis scale factor.
    pub scale: Vec3,
}

impl MagCalibration {
    /// No correction: zero bias, unit scale.
    pub const NONE: Self = Self {
        bias: [0.0; 3],
        scale: [1.0; 3],
    };
}
