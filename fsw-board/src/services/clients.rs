//! Clients: the `fsw-hal` traits, implemented by sending a request to whichever service serves the
//! channel and waiting for the reply.
//!
//! One client per trait, shared by every service: a [`GyroClient`] works the same whether the
//! angular rate comes from the v4 IMU or a different chip. All clients of one task share that
//! task's [`ReplySlot`](super::ReplySlot).

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use fsw_hal::sensors::{Gyro, Magnetometer};
use fsw_hal::{SensorFault, Vec3};

use super::{GyroChannel, GyroRequest, MagChannel, MagRequest, QUEUE_DEPTH, VecSlot};

/// A [`Gyro`] that reads through whichever service serves the gyro channel.
pub struct GyroClient {
    requests: &'static GyroChannel,
    reply: &'static VecSlot,
}

impl GyroClient {
    pub const fn new(requests: &'static GyroChannel, reply: &'static VecSlot) -> Self {
        Self { requests, reply }
    }
}

impl Gyro for GyroClient {
    async fn angular_rate(&mut self) -> Result<Vec3, SensorFault> {
        call(self.requests, self.reply, GyroRequest { reply: self.reply }).await
    }
}

/// A [`Magnetometer`] that reads through whichever service serves the magnetometer channel.
pub struct MagClient {
    requests: &'static MagChannel,
    reply: &'static VecSlot,
}

impl MagClient {
    pub const fn new(requests: &'static MagChannel, reply: &'static VecSlot) -> Self {
        Self { requests, reply }
    }
}

impl Magnetometer for MagClient {
    async fn magnetic_field(&mut self) -> Result<Vec3, SensorFault> {
        call(self.requests, self.reply, MagRequest { reply: self.reply }).await
    }
}

/// Send one request and wait for its reply.
async fn call<R>(
    requests: &'static Channel<CriticalSectionRawMutex, R, QUEUE_DEPTH>,
    reply: &'static VecSlot,
    request: R,
) -> Result<Vec3, SensorFault> {
    requests.send(request).await;
    reply.wait().await
}
