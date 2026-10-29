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
//! Replies go into a [`Slot`] the client passes in with each request, not into a shared inbox, so a
//! reply can only reach the request that asked for it and the service never waits on a client.

pub mod clients;
pub mod imu;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use fsw_hal::{SensorFault, Vec3};

/// Where a service writes its reply: the value, tagged with the request it answers.
///
/// The tag lets a client drop a late reply to an earlier request that timed out. A `Signal` holds
/// only the latest value, and `signal` overwrites it without waiting, so the service never blocks
/// on a slow client. `CriticalSectionRawMutex` because the service and its clients may run on
/// different executors.
pub type Slot<T> = Signal<CriticalSectionRawMutex, (u32, T)>;

/// Reply slot for a vector reading (angular rate, magnetic field).
pub type VecSlot = Slot<Result<Vec3, SensorFault>>;

/// Requests waiting for a service. Room for one request per client is enough, since a client waits
/// for each reply before sending again.
pub const QUEUE_DEPTH: usize = 4;

/// Ask for an angular rate ([`fsw_hal::sensors::Gyro`]).
pub struct GyroRequest {
    pub tag: u32,
    pub reply: &'static VecSlot,
}

/// Ask for a magnetic field ([`fsw_hal::sensors::Magnetometer`]).
pub struct MagRequest {
    pub tag: u32,
    pub reply: &'static VecSlot,
}

pub type GyroChannel = Channel<CriticalSectionRawMutex, GyroRequest, QUEUE_DEPTH>;
pub type MagChannel = Channel<CriticalSectionRawMutex, MagRequest, QUEUE_DEPTH>;
