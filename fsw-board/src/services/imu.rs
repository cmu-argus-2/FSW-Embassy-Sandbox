//! IMU service: the BMX160 and the task that owns it. The task serves both the gyro and the
//! magnetometer channel, reading the driver directly for each request.
//!
//! ```text
//! ADCS task                          IMU service task (run)
//! ─────────                          ──────────────────────
//! GyroClient::angular_rate()
//!   ├─ send GyroRequest{slot} ──────▶ receive from GYRO channel
//!   │                                 imu.gyro().await           (I2C read)
//!   └─ wait on slot ◀──────────────── slot.signal(result)
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
pub type Imu = BMX160<I2c0Device>;

/// Set up the BMX160 on I2C0 with the flight settings. If it fails to start, the driver is still
/// returned and every read reports [`SensorFault`], so the task's failure path runs instead of the
/// board failing to boot.
pub async fn init(i2c: I2c0Device) -> Imu {
    let mut imu = BMX160::new(i2c, addresses::i2c0::IMU);
    match imu.init(&mut Delay).await {
        Ok(()) => info!("BMX160 up, mag trim: {}", imu.trim()),
        Err(e) => error!("BMX160 init failed: {}", e),
    }
    imu
}

// ── Service task ────────────────────────────────────────────────────────────────────────────────

/// Own the IMU and answer gyro and magnetometer requests one at a time, forever.
///
/// A binary wraps this in an `#[embassy_executor::task]` (task functions must live where the
/// concrete types are known) and spawns it on whichever executor the IMU should run on.
pub async fn run(mut imu: Imu, gyro: &'static GyroChannel, mag: &'static MagChannel) -> ! {
    loop {
        match select(gyro.receive(), mag.receive()).await {
            Either::First(GyroRequest { reply }) => {
                let result = imu.gyro().await.map_err(|e| {
                    error!("gyro read failed: {}", e);
                    SensorFault
                });
                reply.signal(result);
            }
            Either::Second(MagRequest { reply }) => {
                let result = imu.mag().await.map_err(|e| {
                    error!("mag read failed: {}", e);
                    SensorFault
                });
                reply.signal(result);
            }
        }
    }
}
