//! The two shared I2C buses.
//!
//! Each bus is wrapped in a `CriticalSectionRawMutex` mutex so devices on it can be used from tasks
//! on different executors (including interrupt executors). Create a device handle with
//! `I2cDevice::new(bus)`, or use the [`I2c0Device`] / [`I2c1Device`] aliases in task signatures.

use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_rp::i2c::{self, Async, I2c};
use embassy_rp::peripherals::{I2C0, I2C1};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use static_cell::StaticCell;

use crate::power::PoweredUp;
use crate::{I2c0Pins, I2c1Pins, Irqs};

/// Bus clock (argus_v4.py: `frequency=400000`). embassy-rp defaults to 100 kHz.
pub const I2C_FREQUENCY_HZ: u32 = 400_000;

/// Whether to enable the RP2350's internal pull-ups on SDA/SCL.
///
/// Disabled because the board has external pull-ups: CircuitPython refuses to create an I2C bus
/// without them ("No pull up found on SDA or SCL"), and the Python flight software runs both buses.
/// embassy-rp enables the internal ones by default.
pub const INTERNAL_PULLUPS: bool = false;

pub type I2c0 = I2c<'static, I2C0, Async>;
pub type I2c1 = I2c<'static, I2C1, Async>;
pub type I2c0Bus = Mutex<CriticalSectionRawMutex, I2c0>;
pub type I2c1Bus = Mutex<CriticalSectionRawMutex, I2c1>;
pub type I2c0Device = I2cDevice<'static, CriticalSectionRawMutex, I2c0>;
pub type I2c1Device = I2cDevice<'static, CriticalSectionRawMutex, I2c1>;

fn config() -> i2c::Config {
    let mut c = i2c::Config::default();
    c.frequency = I2C_FREQUENCY_HZ;
    c.sda_pullup = INTERNAL_PULLUPS;
    c.scl_pullup = INTERNAL_PULLUPS;
    c
}

/// Create the shared I2C0 bus. Can only be called once, since `I2c0Pins` can only be taken once.
pub fn i2c0(r: I2c0Pins, _powered: &PoweredUp) -> &'static I2c0Bus {
    static BUS: StaticCell<I2c0Bus> = StaticCell::new();
    BUS.init(Mutex::new(I2c::new_async(
        r.i2c,
        r.scl,
        r.sda,
        Irqs,
        config(),
    )))
}

/// Create the shared I2C1 bus. Can only be called once, since `I2c1Pins` can only be taken once.
pub fn i2c1(r: I2c1Pins, _powered: &PoweredUp) -> &'static I2c1Bus {
    static BUS: StaticCell<I2c1Bus> = StaticCell::new();
    BUS.init(Mutex::new(I2c::new_async(
        r.i2c,
        r.scl,
        r.sda,
        Irqs,
        config(),
    )))
}
