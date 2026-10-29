//! Every device on the board, brought up in one place.
//!
//! A binary calls [`Hardware::init`], then destructures the result and moves each device into the
//! one task that owns it. Add a field here for each new device.

use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;

use crate::devices::imu::Bmx160Imu;
use crate::power::BoardPower;
use crate::{I2c0Pins, PowerPins, i2c_bus};

pub struct Hardware {
    /// Power lines from the boot sequence. Keep this alive for the whole program: dropping it
    /// releases the enable pins, which switches the lines off (and every device with them).
    pub power: BoardPower,
    /// Gyro (BMX160 on I2C0).
    pub imu: Bmx160Imu,
}

impl Hardware {
    /// Power up the board (`BoardPower::up`), create the buses, and initialise every device.
    pub async fn init(power: PowerPins, i2c0: I2c0Pins) -> Self {
        let power = BoardPower::up(power).await;
        let i2c0 = i2c_bus::i2c0(i2c0, &power.powered_up);

        let imu = Bmx160Imu::new(I2cDevice::new(i2c0)).await;

        Self { power, imu }
    }
}
