//! IMU adapter: the BMX160 as an `fsw-hal` [`Gyro`].
//!
//! The BMX160 axes are used directly as body axes, so there is no axis remapping here.

use defmt::{error, info};
use embassy_time::Delay;
use fsw_hal::sensors::Gyro;
use fsw_hal::{SensorFault, Vec3};
use fsw_lib::drivers::bmx160::BMX160;

use crate::devices;
use crate::i2c_bus::I2c0Device;

pub struct Bmx160Imu {
    driver: BMX160<I2c0Device>,
}

impl Bmx160Imu {
    /// Set up the BMX160 on I2C0 with the flight settings. If it fails to start, the
    /// adapter is still returned and every read reports [`SensorFault`], so the task's failure path
    /// runs instead of the board failing to boot.
    pub async fn new(i2c: I2c0Device) -> Self {
        let mut driver = BMX160::new(i2c, devices::i2c0::IMU);
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
