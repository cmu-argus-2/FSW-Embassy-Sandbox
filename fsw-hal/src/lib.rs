//! Argus hardware abstraction layer.
//!
//! The traits apps use to reach hardware, and the types those traits use. Apps depend only on this
//! crate; `fsw-board` implements the traits for the mainboard (on top of `fsw-lib` drivers), and
//! tests can implement them with fakes.
//!
//! Python equivalent: `flight/hal/cubesat.py` (the `CubeSat` interface), plus the methods apps expect
//! on the objects it returns (`SATELLITE.IMU.gyro()`), which Python never writes down.
//!
//! Traits are grouped by device class, not by app, and every trait states its units and frame:
//!
//! - [`sensors`]: [`sensors::Gyro`].
//! - [`actuators`]: [`actuators::Magnetorquers`].
#![no_std]

pub mod actuators;
pub mod sensors;

/// Time delays, from `embedded-hal-async`. Re-exported so apps get every hardware abstraction,
/// timing included, from this crate.
pub use embedded_hal_async::delay::DelayNs;

/// Vector in the satellite body frame.
pub type Vec3 = [f32; 3];

/// A sensor could not produce a reading (bus error, device fault, ...). Why it failed is the
/// implementation's business; apps only need to know that it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SensorFault;
