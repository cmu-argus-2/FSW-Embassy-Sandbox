//! Every device on the board, brought up in one place.
//!
//! A binary calls [`Hardware::init`], then destructures the result and moves each device into the
//! one task that owns it. Add a field here for each new device.

use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;

use crate::power::BoardPower;
use crate::services::fuel_gauge::{self, Gauge};
use crate::services::imu::{self, Imu};
use crate::services::power_monitor::{self, Monitor};
use crate::{I2c0Pins, I2c1Pins, PowerPins, addresses, i2c_bus};

pub struct Hardware {
    /// Power lines from the boot sequence. Keep this alive for the whole program: dropping it
    /// releases the enable pins, which switches the lines off (and every device with them).
    pub power: BoardPower,
    /// Gyro and magnetometer (BMX160 on I2C0).
    pub imu: Imu,
    /// Mainboard input power monitor (ADM1176 on I2C1).
    pub board_pwr: Monitor,
    /// GPS power monitor (ADM1176 on I2C1).
    pub gps_pwr: Monitor,
    /// Radio power monitor (ADM1176 on I2C1).
    pub radio_pwr: Monitor,
    /// Jetson power monitor (ADM1176 on I2C1).
    pub jetson_pwr: Monitor,
    /// Battery fuel gauge (MAX17205 on I2C1).
    pub fuel_gauge: Gauge,
}

impl Hardware {
    /// Power up the board (`BoardPower::up`), create the buses, and initialise every device.
    pub async fn init(power: PowerPins, i2c0: I2c0Pins, i2c1: I2c1Pins) -> Self {
        use addresses::i2c1::{BOARD_PWR, GPS_PWR, JETSON_PWR, RADIO_PWR};

        let power = BoardPower::up(power).await;
        let i2c0 = i2c_bus::i2c0(i2c0, &power.powered_up);
        let i2c1 = i2c_bus::i2c1(i2c1, &power.powered_up);

        let imu = imu::init(I2cDevice::new(i2c0)).await;
        let board_pwr = power_monitor::init(I2cDevice::new(i2c1), BOARD_PWR).await;
        let gps_pwr = power_monitor::init(I2cDevice::new(i2c1), GPS_PWR).await;
        let radio_pwr = power_monitor::init(I2cDevice::new(i2c1), RADIO_PWR).await;
        let jetson_pwr = power_monitor::init(I2cDevice::new(i2c1), JETSON_PWR).await;
        let fuel_gauge = fuel_gauge::init(I2cDevice::new(i2c1)).await;

        Self {
            power,
            imu,
            board_pwr,
            gps_pwr,
            radio_pwr,
            jetson_pwr,
            fuel_gauge,
        }
    }
}
