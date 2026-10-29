//! IMU service: the BMX160 as an `fsw-hal` [`Gyro`], the task that owns it, and the client other
//! tasks read it through.
//!
//! ```text
//! ADCS task                         IMU service task (run)
//! ─────────                         ──────────────────────
//! ImuClient::angular_rate()
//!   ├─ send AngularRate{tag, slot} ─▶ receive request
//!   │                                imu.angular_rate().await   (I2C read)
//!   └─ wait on slot ◀─────────────── slot.signal((tag, result))
//! ```
//!
//! The BMX160 axes are used directly as body axes, so there is no axis remapping here.

use defmt::{error, info};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Delay, Duration, with_timeout};
use fsw_hal::sensors::Gyro;
use fsw_hal::{SensorFault, Vec3};
use fsw_lib::drivers::bmx160::BMX160;

use super::Slot;
use crate::addresses;
use crate::i2c_bus::I2c0Device;

// ── Device ──────────────────────────────────────────────────────────────────────────────────────

/// The BMX160 on I2C0.
pub struct Bmx160Imu {
    driver: BMX160<I2c0Device>,
}

impl Bmx160Imu {
    /// Set up the BMX160 on I2C0 with the flight settings. If it fails to start, the device is
    /// still returned and every read reports [`SensorFault`], so the task's failure path runs
    /// instead of the board failing to boot.
    pub async fn new(i2c: I2c0Device) -> Self {
        let mut driver = BMX160::new(i2c, addresses::i2c0::IMU);
        match driver.init(&mut Delay).await {
            Ok(()) => info!("BMX160 up, mag trim: {}", driver.trim()),
            Err(e) => error!("BMX160 init failed: {}", e),
        }
        Self { driver }
    }
}

impl Gyro for Bmx160Imu {
    async fn angular_rate(&mut self) -> Result<Vec3, SensorFault> {
        self.driver.gyro().await.map_err(|e| {
            error!("gyro read failed: {}", e);
            SensorFault
        })
    }
}

// ── Service task ────────────────────────────────────────────────────────────────────────────────

/// Reply slot for a gyro read.
pub type RateSlot = Slot<Result<Vec3, SensorFault>>;

/// What a client can ask the IMU service for. Each request carries the slot to reply into.
pub enum ImuRequest {
    AngularRate { tag: u32, reply: &'static RateSlot },
}

/// Requests waiting for the IMU service. Room for one request per client is enough, since a client
/// waits for each reply before sending again.
pub type ImuChannel = Channel<CriticalSectionRawMutex, ImuRequest, 4>;

/// Own the IMU and answer requests one at a time, forever.
///
/// A binary wraps this in an `#[embassy_executor::task]` (task functions must live where the
/// concrete types are known) and spawns it on whichever executor the IMU should run on.
pub async fn run(mut imu: Bmx160Imu, requests: &'static ImuChannel) -> ! {
    loop {
        match requests.receive().await {
            ImuRequest::AngularRate { tag, reply } => {
                reply.signal((tag, imu.angular_rate().await));
            }
        }
    }
}

// ── Client ──────────────────────────────────────────────────────────────────────────────────────

/// How long a client waits for a reply before reporting [`SensorFault`]. A gyro read is one short
/// I2C transfer (well under 1 ms), so this only fires if the service is stuck.
pub const REPLY_TIMEOUT: Duration = Duration::from_millis(20);

/// A [`Gyro`] that reads through the IMU service. Each client needs its own reply slot.
pub struct ImuClient {
    requests: &'static ImuChannel,
    reply: &'static RateSlot,
    /// Tag of the last request sent; replies with any other tag are stale.
    tag: u32,
}

impl ImuClient {
    pub const fn new(requests: &'static ImuChannel, reply: &'static RateSlot) -> Self {
        Self {
            requests,
            reply,
            tag: 0,
        }
    }
}

impl Gyro for ImuClient {
    async fn angular_rate(&mut self) -> Result<Vec3, SensorFault> {
        self.tag = self.tag.wrapping_add(1);
        let tag = self.tag;

        self.requests
            .send(ImuRequest::AngularRate {
                tag,
                reply: self.reply,
            })
            .await;

        // Wait for the reply to this request, skipping any late reply to an earlier one.
        let reply = async {
            loop {
                let (t, result) = self.reply.wait().await;
                if t == tag {
                    return result;
                }
            }
        };
        with_timeout(REPLY_TIMEOUT, reply)
            .await
            .unwrap_or(Err(SensorFault))
    }
}
