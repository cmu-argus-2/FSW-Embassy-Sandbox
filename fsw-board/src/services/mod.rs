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

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use fsw_hal::{SensorFault, Vec3};

/// Where services write their replies to one task: the value, tagged with the request it answers.
///
/// One per task, shared by all of that task's clients. The task must send one request at a time
/// (each client call finishes before the next starts), which is how a task's sequential code works
/// anyway; two calls running at once (e.g. with `join`) would wait on the same slot.
///
/// The tag lets a client drop a late reply to an earlier request that timed out. Tags come from
/// the slot, not the client, so they are unique across every client sharing it: a late gyro reply
/// can never be taken as the answer to a magnetometer request. A `Signal` holds only the latest
/// value, and writing overwrites it without waiting, so a service never blocks on a slow task.
/// `CriticalSectionRawMutex` because the service and the task may run on different executors.
pub struct ReplySlot<T> {
    signal: Signal<CriticalSectionRawMutex, (u32, T)>,
    next_tag: AtomicU32,
}

impl<T: Send> ReplySlot<T> {
    pub const fn new() -> Self {
        Self {
            signal: Signal::new(),
            next_tag: AtomicU32::new(0),
        }
    }

    /// Write the reply to the request with this tag (called by services).
    pub fn reply(&self, tag: u32, value: T) {
        self.signal.signal((tag, value));
    }

    /// A tag for a new request, different from the last 2^32 handed out.
    fn new_tag(&self) -> u32 {
        self.next_tag
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
    }

    /// Wait for the reply to the request with this tag, dropping late replies to earlier ones.
    async fn wait_for(&self, tag: u32) -> T {
        loop {
            let (t, value) = self.signal.wait().await;
            if t == tag {
                return value;
            }
        }
    }
}

impl<T: Send> Default for ReplySlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Reply slot for vector readings (angular rate, magnetic field).
pub type VecSlot = ReplySlot<Result<Vec3, SensorFault>>;

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
