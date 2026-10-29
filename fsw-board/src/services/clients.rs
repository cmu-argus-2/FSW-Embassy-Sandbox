//! Clients: the `fsw-hal` traits, implemented by sending a request to whichever service serves the
//! channel and waiting for the reply.
//!
//! One client per trait, shared by every service: a [`GyroClient`] works the same whether the
//! angular rate comes from the v4 IMU or a different chip. Each client needs its own reply slot.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, with_timeout};
use fsw_hal::sensors::{Gyro, Magnetometer};
use fsw_hal::{SensorFault, Vec3};

use super::{GyroChannel, GyroRequest, MagChannel, MagRequest, QUEUE_DEPTH, VecSlot};

/// How long a client waits for a reply before reporting [`SensorFault`]. A read is one short I2C
/// transfer (well under 1 ms), so this only fires if the service is stuck.
pub const REPLY_TIMEOUT: Duration = Duration::from_millis(20);

/// A [`Gyro`] that reads through whichever service serves the gyro channel.
pub struct GyroClient {
    requests: &'static GyroChannel,
    reply: &'static VecSlot,
    /// Tag of the last request sent; replies with any other tag are stale.
    tag: u32,
}

impl GyroClient {
    pub const fn new(requests: &'static GyroChannel, reply: &'static VecSlot) -> Self {
        Self {
            requests,
            reply,
            tag: 0,
        }
    }
}

impl Gyro for GyroClient {
    async fn angular_rate(&mut self) -> Result<Vec3, SensorFault> {
        call(self.requests, self.reply, &mut self.tag, |tag, reply| {
            GyroRequest { tag, reply }
        })
        .await
    }
}

/// A [`Magnetometer`] that reads through whichever service serves the magnetometer channel.
pub struct MagClient {
    requests: &'static MagChannel,
    reply: &'static VecSlot,
    /// Tag of the last request sent; replies with any other tag are stale.
    tag: u32,
}

impl MagClient {
    pub const fn new(requests: &'static MagChannel, reply: &'static VecSlot) -> Self {
        Self {
            requests,
            reply,
            tag: 0,
        }
    }
}

impl Magnetometer for MagClient {
    async fn magnetic_field(&mut self) -> Result<Vec3, SensorFault> {
        call(self.requests, self.reply, &mut self.tag, |tag, reply| {
            MagRequest { tag, reply }
        })
        .await
    }
}

/// Send one request (built by `request` from a new tag and the reply slot) and wait for its reply,
/// skipping any late reply to an earlier request.
async fn call<R>(
    requests: &'static Channel<CriticalSectionRawMutex, R, QUEUE_DEPTH>,
    reply: &'static VecSlot,
    tag: &mut u32,
    request: impl FnOnce(u32, &'static VecSlot) -> R,
) -> Result<Vec3, SensorFault> {
    *tag = tag.wrapping_add(1);
    let tag = *tag;

    requests.send(request(tag, reply)).await;

    let wait = async {
        loop {
            let (t, result) = reply.wait().await;
            if t == tag {
                return result;
            }
        }
    };
    with_timeout(REPLY_TIMEOUT, wait)
        .await
        .unwrap_or(Err(SensorFault))
}
