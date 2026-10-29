//! IMU service: the BMX160 and the task that owns it. The task serves both the gyro and the
//! magnetometer channel, reading the driver directly for each request.
//!
//! ```text
//! ADCS task                          IMU service task (run)
//! ─────────                          ──────────────────────
//! GyroClient::angular_rate()
//!   ├─ send GyroRequest{tag, slot} ─▶ receive from GYRO channel
//!   │                                 driver.gyro().await        (I2C read)
//!   └─ wait on slot ◀──────────────── slot.signal((tag, result))
//! ```
//!
//! The BMX160 axes are used directly as body axes, so there is no axis remapping here.

use defmt::{error, info};
use embassy_futures::select::{Either, select};
use embassy_time::Delay;
use fsw_hal::SensorFault;
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

// ── Service task ────────────────────────────────────────────────────────────────────────────────

/// Own the IMU and answer gyro and magnetometer requests one at a time, forever.
///
/// A binary wraps this in an `#[embassy_executor::task]` (task functions must live where the
/// concrete types are known) and spawns it on whichever executor the IMU should run on.
pub async fn run(imu: Bmx160Imu, gyro: &'static GyroChannel, mag: &'static MagChannel) -> ! {
    let mut driver = imu.driver;
    loop {
        match select(gyro.receive(), mag.receive()).await {
            Either::First(GyroRequest { tag, reply }) => {
                let result = driver.gyro().await.map_err(|e| {
                    error!("gyro read failed: {}", e);
                    SensorFault
                });
                reply.signal((tag, result));
            }
            Either::Second(MagRequest { tag, reply }) => {
                let result = driver.mag().await.map_err(|e| {
                    error!("mag read failed: {}", e);
                    SensorFault
                });
                reply.signal((tag, result));
            }
        }
    }
}
