//! Device services: one task per device that owns it and serves requests from other tasks.
//!
//! Each service file has three parts:
//!
//! - a request enum, sent over the service's channel;
//! - the service loop, which owns the device and answers each request;
//! - a client, which implements the device's `fsw-hal` trait by sending a request and waiting for
//!   the reply. Apps take the client in place of the device and do not know the difference.
//!
//! Replies go into a [`Slot`] the client passes in with each request, not into a shared inbox, so a
//! reply can only reach the request that asked for it and the service never waits on a client.

pub mod imu;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

/// Where a service writes its reply: the value, tagged with the request it answers.
///
/// The tag lets a client drop a late reply to an earlier request that timed out. A `Signal` holds
/// only the latest value, and `signal` overwrites it without waiting, so the service never blocks
/// on a slow client. `CriticalSectionRawMutex` because the service and its clients may run on
/// different executors.
pub type Slot<T> = Signal<CriticalSectionRawMutex, (u32, T)>;
