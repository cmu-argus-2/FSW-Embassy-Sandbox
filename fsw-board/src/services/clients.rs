//! Clients: the `fsw-hal` traits, implemented by sending a request to whichever service serves the
//! channel and waiting for the reply.
//!
//! One client per trait, shared by every service: a [`GyroClient`] works the same whether the
//! angular rate comes from the v4 IMU or a different chip. All clients of one task that get the
//! same reply type share that task's [`ReplySlot`].

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use fsw_hal::sensors::{BatteryReading, FuelGauge, Gyro, Magnetometer, PowerMonitor, PowerReading};
use fsw_hal::{SensorFault, Vec3};

use super::{
    BatteryChannel, BatteryRequest, BatterySlot, GyroChannel, GyroRequest, MagChannel, MagRequest,
    PowerChannel, PowerRequest, PowerSlot, QUEUE_DEPTH, ReplySlot, VecSlot,
};

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

/// A [`PowerMonitor`] that reads through whichever service serves one rail's power channel.
pub struct PowerMonitorClient {
    requests: &'static PowerChannel,
    reply: &'static PowerSlot,
}

impl PowerMonitorClient {
    pub const fn new(requests: &'static PowerChannel, reply: &'static PowerSlot) -> Self {
        Self { requests, reply }
    }
}

impl PowerMonitor for PowerMonitorClient {
    async fn power(&mut self) -> Result<PowerReading, SensorFault> {
        call(
            self.requests,
            self.reply,
            PowerRequest { reply: self.reply },
        )
        .await
    }
}

/// A [`FuelGauge`] that reads through whichever service serves the battery channel.
pub struct FuelGaugeClient {
    requests: &'static BatteryChannel,
    reply: &'static BatterySlot,
}

impl FuelGaugeClient {
    pub const fn new(requests: &'static BatteryChannel, reply: &'static BatterySlot) -> Self {
        Self { requests, reply }
    }
}

impl FuelGauge for FuelGaugeClient {
    async fn battery(&mut self) -> Result<BatteryReading, SensorFault> {
        call(
            self.requests,
            self.reply,
            BatteryRequest { reply: self.reply },
        )
        .await
    }
}

/// Send one request and wait for its reply.
async fn call<R, T: Send>(
    requests: &'static Channel<CriticalSectionRawMutex, R, QUEUE_DEPTH>,
    reply: &'static ReplySlot<T>,
    request: R,
) -> T {
    requests.send(request).await;
    reply.wait().await
}
