//! Device services: one file per device, each with the device and the task that owns it.
//!
//! Each service file has two parts:
//!
//! - the device: an `fsw-lib` driver set up with this board's settings;
//! - the service task: a `run` loop that owns the device and answers requests by calling the
//!   driver, turning its errors into the `fsw-hal` fault types.
//!
//! Requests are defined once per `fsw-hal` trait, not per chip: [`GyroRequest`] asks for an angular
//! rate whichever chip provides it. A service serves the channel of each trait its device provides
//! (the v4 IMU serves both [`GyroChannel`] and [`MagChannel`]), and the binary decides which service
//! serves which channel. Apps reach a service through the matching client in [`clients`], which
//! implements the trait, so they never see the messages.
//!
//! Replies go into the requesting task's [`ReplySlot`], passed in with each request. A task needs
//! only one slot, shared by all its clients, because it waits for each reply before sending its
//! next request; the service writes into it without waiting, so it never blocks on a slow task.

pub mod clients;
pub mod imu;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use fsw_hal::{SensorFault, Vec3};

/// Where services write their replies to one task. One per task, shared by all of that task's
/// clients.
///
/// A `Signal` holds one value and writing overwrites it without waiting, so a service never blocks
/// on a slow task. That is enough because a task sends one request at a time and waits for its
/// reply before sending the next.
pub type ReplySlot<T> = Signal<CriticalSectionRawMutex, T>;

/// Reply slot for vector readings (angular rate, magnetic field).
pub type VecSlot = ReplySlot<Result<Vec3, SensorFault>>;

/// Requests waiting for a service. Room for one request per client is enough, since a client waits
/// for each reply before sending again.
pub const QUEUE_DEPTH: usize = 4;

/// Ask for an angular rate ([`fsw_hal::sensors::Gyro`]), replied to in `reply`.
pub struct GyroRequest {
    pub reply: &'static VecSlot,
}

/// Ask for a magnetic field ([`fsw_hal::sensors::Magnetometer`]), replied to in `reply`.
pub struct MagRequest {
    pub reply: &'static VecSlot,
}

pub type GyroChannel = Channel<CriticalSectionRawMutex, GyroRequest, QUEUE_DEPTH>;
pub type MagChannel = Channel<CriticalSectionRawMutex, MagRequest, QUEUE_DEPTH>;
