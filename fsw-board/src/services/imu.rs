//! IMU service: the BMX160 as an `fsw-hal` [`Gyro`] and [`Magnetometer`], and the task that owns it.
//! The task serves both the gyro and the magnetometer channel.
//!
//! ```text
//! ADCS task                          IMU service task (run)
//! ─────────                          ──────────────────────
//! GyroClient::angular_rate()
//!   ├─ send GyroRequest{tag, slot} ─▶ receive from GYRO channel
//!   │                                 imu.angular_rate().await   (I2C read)
//!   └─ wait on slot ◀──────────────── slot.signal((tag, result))
//! ```
//!
//! The BMX160 axes are used directly as body axes, so there is no axis remapping here.

use defmt::{error, info};
use embassy_futures::select::{Either, select};
use embassy_time::Delay;
use fsw_hal::sensors::{Gyro, Magnetometer};
use fsw_hal::{SensorFault, Vec3};
use fsw_lib::drivers::bmx160::BMX160;

use super::{GyroChannel, GyroRequest, MagChannel, MagRequest};
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

impl Magnetometer for Bmx160Imu {
    async fn magnetic_field(&mut self) -> Result<Vec3, SensorFault> {
        self.driver.mag().await.map_err(|e| {
            error!("mag read failed: {}", e);
            SensorFault
        })
    }
}

// ── Service task ────────────────────────────────────────────────────────────────────────────────

/// Own the IMU and answer gyro and magnetometer requests one at a time, forever.
///
/// A binary wraps this in an `#[embassy_executor::task]` (task functions must live where the
/// concrete types are known) and spawns it on whichever executor the IMU should run on.
pub async fn run(mut imu: Bmx160Imu, gyro: &'static GyroChannel, mag: &'static MagChannel) -> ! {
    loop {
        match select(gyro.receive(), mag.receive()).await {
            Either::First(GyroRequest { tag, reply }) => {
                reply.signal((tag, imu.angular_rate().await));
            }
            Either::Second(MagRequest { tag, reply }) => {
                reply.signal((tag, imu.magnetic_field().await));
            }
        }
    }
}
